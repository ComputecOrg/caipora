//! Treino da rede do Caipora com o `bullet` (github.com/jw1912/bullet, licença MIT), na GPU.
//!
//! A arquitetura e a quantização precisam bater com `src/nnue.rs` da engine:
//! (768 → 256)×2 → 1, SCReLU, QA = 255, QB = 64, escala 400.
//!
//! Uso:
//! `caipora-trainer <dados.bin> <id> <superbatches> [lotes por superbatch] [wdl] [lr inicial]`
//!
//! - Os dados vêm do `caipora datagen`, convertidos com `bullet-utils convert --from text` e
//!   embaralhados com `bullet-utils shuffle`.
//! - O alvo de cada posição mistura o resultado da partida (peso `wdl`) com a pontuação da busca
//!   (peso `1 - wdl`).
//! - A taxa de aprendizado cai em cosseno até 1% da inicial.

use bullet::{
    game::inputs::Chess768,
    nn::optimiser::AdamW,
    trainer::{
        save::SavedFormat,
        schedule::{TrainingSchedule, TrainingSteps, lr, wdl},
        settings::LocalSettings,
    },
    value::{ValueTrainerBuilder, loader},
};

const HIDDEN: usize = 256;
const SCALE: i32 = 400;
const QA: i16 = 255;
const QB: i16 = 64;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "uso: caipora-trainer <dados.bin> <id> <superbatches> [lotes] [wdl] [lr]";
    let data = args.first().expect(usage).clone();
    let net_id = args.get(1).expect(usage).clone();
    let superbatches: usize = args.get(2).expect(usage).parse().expect(usage);
    let batches: usize = args.get(3).map_or(6104, |v| v.parse().expect(usage));
    let wdl_weight: f32 = args.get(4).map_or(0.5, |v| v.parse().expect(usage));
    let initial_lr: f32 = args.get(5).map_or(0.001, |v| v.parse().expect(usage));

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
            let l0 = builder.new_affine("l0", 768, HIDDEN);
            let l1 = builder.new_affine("l1", 2 * HIDDEN, 1);
            let stm_hidden = l0.forward(stm).screlu();
            let ntm_hidden = l0.forward(ntm).screlu();
            l1.forward(stm_hidden.concat(ntm_hidden))
        });

    let schedule = TrainingSchedule {
        net_id,
        eval_scale: SCALE as f32,
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
    let data_loader = loader::DirectSequentialDataLoader::new(&[data.as_str()]);
    trainer.run(&schedule, &settings, &data_loader);
}
