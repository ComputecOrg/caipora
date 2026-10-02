//! Treino da rede do Caipora com o `bullet` (github.com/jw1912/bullet, licença MIT), na GPU.
//!
//! A arquitetura e a quantização precisam bater com `src/nnue.rs` da engine:
//! (768·8 king buckets espelhados → HIDDEN)×2 → 8 output buckets, SCReLU, QA = 255, QB = 64,
//! escala 400. O layout dos king buckets (`KING_BUCKETS`) é o mesmo da engine; um factoriser
//! (pesos comuns a todos os buckets) ajuda no treino e é somado a cada bucket ao salvar. HIDDEN vem
//! da variável de ambiente `CAIPORA_HIDDEN` (padrão 1024, o da engine).
//!
//! Uso:
//! `caipora-trainer <dados.bin|a.binpack[,b.binpack...]> <id> <superbatches> [lotes] [wdl] [lr]`
//!
//! - Os dados vêm do `caipora datagen`, convertidos com `bullet-utils convert --from text` e
//!   embaralhados com `bullet-utils shuffle`; ou de binpacks do Stockfish (dados do Lc0, ODbL,
//!   D20), lidos direto e intercalados bloco a bloco (`interleave`), só com as posições calmas
//!   (`quiet_position`).
//! - `CAIPORA_EVAL_SCALE` (padrão 400) divide a pontuação dos dados antes da sigmoide. Os binpacks
//!   estão na escala interna do Stockfish: a escala certa sai de `dump` + `caipora validate` (a
//!   que deixa a rede atual mais perto dos dados), para a rede nova continuar em centipeões nossos.
//! - O alvo de cada posição mistura o resultado da partida (peso `wdl`) com a pontuação da busca
//!   (peso `1 - wdl`).
//! - A taxa de aprendizado cai em cosseno até 1% da inicial.
//!
//! Segundo estágio: `CAIPORA_RESUME=<checkpoint>` continua o treino de um checkpoint.
//! Conferência: `caipora-trainer eval <checkpoint> <FEN>...` imprime a saída da rede em ponto
//! flutuante (centipeões, do lado a jogar), para comparar com o `eval` da engine.
//!
//! Amostra: `caipora-trainer dump <dados.binpack> <quantas> <saída.txt> [a cada N]` grava posições
//! calmas no formato do `caipora datagen` (`FEN | pontuação | resultado`, do lado das brancas).

mod interleave;

use std::io::{BufReader, BufWriter, Write};

use bullet::{
    game::{
        inputs::{ChessBucketsMirrored, get_num_buckets},
        outputs::MaterialCount,
    },
    nn::{
        InitSettings, Shape,
        optimiser::{AdamW, AdamWParams},
    },
    trainer::{
        save::SavedFormat,
        schedule::{TrainingSchedule, TrainingSteps, lr, wdl},
        settings::LocalSettings,
    },
    value::{
        ValueTrainerBuilder,
        loader::{
            self,
            sfbinpack::{MoveType, PieceType, TrainingDataEntry},
        },
    },
};
use sfbinpack::{ChunkReader, read_chunk_into};

const DEFAULT_HIDDEN: usize = 1024;
/// O mesmo layout de `KING_BUCKETS` em `src/nnue.rs` da engine.
#[rustfmt::skip]
const KING_BUCKETS: [usize; 32] = [
    0, 1, 2, 3,
    4, 4, 5, 5,
    6, 6, 6, 6,
    6, 6, 6, 6,
    7, 7, 7, 7,
    7, 7, 7, 7,
    7, 7, 7, 7,
    7, 7, 7, 7,
];
const INPUT_BUCKETS: usize = get_num_buckets(&KING_BUCKETS);
const OUTPUT_BUCKETS: usize = 8;
const SCALE: i32 = 400;
const QA: i16 = 255;
const QB: i16 = 64;

/// Posição que entra no treino: fora da abertura (16 meios-lances), sem xeque, com pontuação
/// finita e com o lance jogado sendo quieto (sem captura nem promoção), como na busca quiescente.
fn quiet_position(entry: &TrainingDataEntry) -> bool {
    entry.ply >= 16
        && !entry.pos.is_checked(entry.pos.side_to_move())
        && entry.score.unsigned_abs() <= 10_000
        && entry.mv.mtype() == MoveType::Normal
        && entry.pos.piece_at(entry.mv.to()).piece_type() == PieceType::None
}

/// Grava `count` posições calmas do binpack (uma a cada `every`) no formato do datagen.
fn dump(path: &str, count: usize, out: &str, every: usize) {
    let mut reader = BufReader::new(std::fs::File::open(path).expect("binpack não abre"));
    let mut writer = BufWriter::new(std::fs::File::create(out).expect("saída não abre"));
    let (mut chunk, mut seen, mut written) = (Vec::new(), 0usize, 0usize);
    while written < count && read_chunk_into(&mut reader, &mut chunk).expect("binpack com defeito")
    {
        let mut entries = ChunkReader::default();
        while written < count && entries.has_next(&chunk) {
            let entry = entries.next(&chunk);
            if !quiet_position(&entry) {
                continue;
            }
            seen += 1;
            if seen % every != 0 {
                continue;
            }
            // O binpack guarda pontuação e resultado do lado a jogar; o datagen, das brancas.
            let white = entry.pos.side_to_move().ordinal() == 0;
            let score = if white { entry.score } else { -entry.score };
            let result = f32::from(1 + entry.result * if white { 1 } else { -1 }) / 2.0;
            let fen = entry.pos.fen().expect("posição sem FEN");
            writeln!(writer, "{fen} | {score} | {result:.1}").expect("falha ao gravar");
            written += 1;
        }
    }
    println!("{written} posições gravadas em {out}");
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("dump") {
        let usage = "uso: caipora-trainer dump <dados.binpack> <quantas> <saída.txt> [a cada N]";
        let count = args.get(2).and_then(|v| v.parse().ok()).expect(usage);
        let every = args.get(4).map_or(1, |v| v.parse().expect(usage));
        dump(
            args.get(1).expect(usage),
            count,
            args.get(3).expect(usage),
            every,
        );
        return;
    }
    let eval_scale: f32 = std::env::var("CAIPORA_EVAL_SCALE").map_or(SCALE as f32, |v| {
        v.parse().expect("CAIPORA_EVAL_SCALE precisa ser um número")
    });
    let usage = "uso: caipora-trainer <dados.bin> <id> <superbatches> [lotes] [wdl] [lr]
                 ou:  caipora-trainer eval <checkpoint> <FEN>...";

    let hidden: usize = std::env::var("CAIPORA_HIDDEN").map_or(DEFAULT_HIDDEN, |v| {
        v.parse().expect("CAIPORA_HIDDEN precisa ser um número")
    });
    println!("camada oculta: {hidden}");
    let mut trainer = ValueTrainerBuilder::default()
        .dual_perspective()
        .optimiser(AdamW)
        .inputs(ChessBucketsMirrored::new(KING_BUCKETS))
        .output_buckets(MaterialCount::<OUTPUT_BUCKETS>)
        .save_format(&[
            // O factoriser é somado a cada bucket: a engine só vê os pesos finais.
            SavedFormat::id("l0w")
                .transform(|store, weights| {
                    let factoriser = store.get("l0f").values.f32().repeat(INPUT_BUCKETS);
                    weights
                        .into_iter()
                        .zip(factoriser)
                        .map(|(a, b)| a + b)
                        .collect()
                })
                .round()
                .quantise::<i16>(QA),
            SavedFormat::id("l0b").round().quantise::<i16>(QA),
            // Transposto: 2·HIDDEN pesos seguidos por bucket de saída, como a engine lê.
            SavedFormat::id("l1w")
                .round()
                .quantise::<i16>(QB)
                .transpose(),
            SavedFormat::id("l1b").round().quantise::<i16>(QA * QB),
        ])
        .loss_fn(|output, target| output.sigmoid().squared_error(target))
        .build(|builder, stm, ntm, output_buckets| {
            let factoriser =
                builder.new_weights("l0f", Shape::new(hidden, 768), InitSettings::Zeroed);
            let mut l0 = builder.new_affine("l0", 768 * INPUT_BUCKETS, hidden);
            l0.weights = l0.weights + factoriser.repeat(INPUT_BUCKETS);
            let l1 = builder.new_affine("l1", 2 * hidden, OUTPUT_BUCKETS);
            let stm_hidden = l0.forward(stm).screlu();
            let ntm_hidden = l0.forward(ntm).screlu();
            l1.forward(stm_hidden.concat(ntm_hidden))
                .select(output_buckets)
        });
    // Pesos de entrada somam o factoriser: limite menor para a soma continuar no intervalo.
    let stricter = AdamWParams {
        max_weight: 0.99,
        min_weight: -0.99,
        ..Default::default()
    };
    trainer.optimiser.set_params_for_weight("l0w", stricter);
    trainer.optimiser.set_params_for_weight("l0f", stricter);

    if args.first().map(String::as_str) == Some("eval") {
        let checkpoint = args.get(1).expect(usage);
        trainer.load_from_checkpoint(checkpoint);
        for fen in &args[2..] {
            let output = trainer.eval_raw_output(fen)[0] * SCALE as f32;
            println!("{output:.1} | {fen}");
        }
        return;
    }

    let data = args.first().expect(usage).clone();
    let net_id = args.get(1).expect(usage).clone();
    let superbatches: usize = args.get(2).expect(usage).parse().expect(usage);
    let batches: usize = args.get(3).map_or(6104, |v| v.parse().expect(usage));
    let wdl_weight: f32 = args.get(4).map_or(0.5, |v| v.parse().expect(usage));
    let initial_lr: f32 = args.get(5).map_or(0.001, |v| v.parse().expect(usage));

    let schedule = TrainingSchedule {
        net_id,
        eval_scale,
        steps: TrainingSteps {
            batch_size: 16_384,
            batches_per_superbatch: batches,
            start_superbatch: 1,
            end_superbatch: superbatches,
        },
        wdl_scheduler: wdl::ConstantWDL { value: wdl_weight },
        lr_scheduler: lr::CosineDecayLR {
            initial_lr,
            final_lr: initial_lr * 0.01,
            final_superbatch: superbatches,
        },
        save_rate: superbatches.div_ceil(4).max(1),
    };
    let settings = LocalSettings {
        threads: 4,
        test_set: None,
        output_directory: "checkpoints",
        batch_queue_size: 64,
    };
    println!("escala dos dados: {eval_scale}");
    // Segundo estágio: continua de um checkpoint (pesos e estado do otimizador), em geral com wdl
    // maior e lr menor que o primeiro.
    if let Ok(checkpoint) = std::env::var("CAIPORA_RESUME") {
        trainer.load_from_checkpoint(&checkpoint);
        println!("continua de {checkpoint}");
    }
    if data.ends_with(".binpack") {
        // Vários binpacks separados por vírgula, intercalados bloco a bloco e embaralhados em lotes
        // de 16 milhões de posições (~512 MB), sem cópia em disco.
        let paths: Vec<&str> = data.split(',').collect();
        let data_loader = interleave::InterleavedBinpacks::new(&paths, 1 << 24, quiet_position);
        trainer.run(&schedule, &settings, &data_loader);
    } else {
        let data_loader = loader::DirectSequentialDataLoader::new(&[data.as_str()]);
        trainer.run(&schedule, &settings, &data_loader);
    }
}
