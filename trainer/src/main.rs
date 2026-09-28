//! Treino da rede do Caipora com o `bullet` (github.com/jw1912/bullet, licença MIT), na GPU.
//!
//! A arquitetura e a quantização precisam bater com `src/nnue.rs` da engine:
//! (768 → HIDDEN)×2 → 1, SCReLU, QA = 255, QB = 64, escala 400. HIDDEN vem da variável de
//! ambiente `CAIPORA_HIDDEN` (padrão 512, o da engine); a engine precisa ser compilada com o
//! mesmo `HIDDEN`.
//!
//! Uso:
//! `caipora-trainer <dados.bin|dados.binpack> <id> <superbatches> [lotes por superbatch] [wdl] [lr]`
//!
//! - Os dados vêm do `caipora datagen`, convertidos com `bullet-utils convert --from text` e
//!   embaralhados com `bullet-utils shuffle`; ou de um binpack do Stockfish (dados do Lc0, ODbL,
//!   D20), lido direto, só com as posições calmas (`quiet_position`).
//! - `CAIPORA_EVAL_SCALE` (padrão 400) divide a pontuação dos dados antes da sigmoide. Os binpacks
//!   estão na escala interna do Stockfish: a escala certa sai de `dump` + `caipora validate` (a
//!   que deixa a rede atual mais perto dos dados), para a rede nova continuar em centipeões nossos.
//! - O alvo de cada posição mistura o resultado da partida (peso `wdl`) com a pontuação da busca
//!   (peso `1 - wdl`).
//! - A taxa de aprendizado cai em cosseno até 1% da inicial.
//!
//! Conferência: `caipora-trainer eval <checkpoint> <FEN>...` imprime a saída da rede em ponto
//! flutuante (centipeões, do lado a jogar), para comparar com o `eval` da engine.
//!
//! Amostra: `caipora-trainer dump <dados.binpack> <quantas> <saída.txt> [a cada N]` grava posições
//! calmas no formato do `caipora datagen` (`FEN | pontuação | resultado`, do lado das brancas).

use std::io::{BufReader, BufWriter, Write};

use bullet::{
    game::inputs::Chess768,
    nn::optimiser::AdamW,
    trainer::{
        save::SavedFormat,
        schedule::{TrainingSchedule, TrainingSteps, lr, wdl},
        settings::LocalSettings,
    },
    value::{
        ValueTrainerBuilder,
        loader::{
            self,
            sfbinpack::{MoveType, PieceType, SfBinpackLoader, TrainingDataEntry},
        },
    },
};
use sfbinpack::{ChunkReader, read_chunk_into};

const DEFAULT_HIDDEN: usize = 512;
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
    while written < count && read_chunk_into(&mut reader, &mut chunk).expect("binpack com defeito") {
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
        dump(args.get(1).expect(usage), count, args.get(3).expect(usage), every);
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
        .inputs(Chess768)
        .save_format(&[
            SavedFormat::id("l0w").round().quantise::<i16>(QA),
            SavedFormat::id("l0b").round().quantise::<i16>(QA),
            SavedFormat::id("l1w").round().quantise::<i16>(QB),
            SavedFormat::id("l1b").round().quantise::<i16>(QA * QB),
        ])
        .loss_fn(|output, target| output.sigmoid().squared_error(target))
        .build(|builder, stm, ntm| {
            let l0 = builder.new_affine("l0", 768, hidden);
            let l1 = builder.new_affine("l1", 2 * hidden, 1);
            let stm_hidden = l0.forward(stm).screlu();
            let ntm_hidden = l0.forward(ntm).screlu();
            l1.forward(stm_hidden.concat(ntm_hidden))
        });

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
    if data.ends_with(".binpack") {
        let data_loader = SfBinpackLoader::new(&data, 1024, 4, quiet_position);
        trainer.run(&schedule, &settings, &data_loader);
    } else {
        let data_loader = loader::DirectSequentialDataLoader::new(&[data.as_str()]);
        trainer.run(&schedule, &settings, &data_loader);
    }
}
