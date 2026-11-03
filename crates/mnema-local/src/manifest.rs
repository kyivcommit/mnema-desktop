//! What each model consists of, pinned: repository, commit, and for every file
//! its size and sha256. The values were taken once from the Hub at the pinned
//! commit (`curl -sL …/resolve/<commit>/<file> | shasum -a 256`).

use crate::ModelId;

/// Our flat Gemma E2B `config.json`; its upstream one is never downloaded. E4B
/// takes its upstream config as is: the pinned `mlx-swift-lm` reads the nested
/// `text_config` (measured 2026-11-03).
const GEMMA_CONFIG: &[u8] = include_bytes!("../assets/gemma-config.json");

#[derive(Debug, Clone)]
pub struct FileSpec {
    pub name: String,
    pub size: u64,
    pub sha256: String,
    /// Written from these bytes instead of downloaded.
    pub bundled: Option<&'static [u8]>,
}

impl FileSpec {
    pub fn new(name: &str, size: u64, sha256: &str) -> Self {
        Self {
            name: name.into(),
            size,
            sha256: sha256.into(),
            bundled: None,
        }
    }
    pub fn bundled(name: &str, bytes: &'static [u8], sha256: &str) -> Self {
        Self {
            bundled: Some(bytes),
            ..Self::new(name, bytes.len() as u64, sha256)
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModelSpec {
    pub repo: String,
    pub commit: String,
    pub files: Vec<FileSpec>,
}

impl ModelSpec {
    /// `<repo-name>@<commit>`, the directory under the store root.
    pub fn dir_name(&self) -> String {
        let name = self.repo.rsplit('/').next().unwrap_or(&self.repo);
        format!("{name}@{}", self.commit)
    }
}

#[derive(Debug, Clone)]
pub struct Manifest {
    pub embed: ModelSpec,
    pub chat: ModelSpec,
    pub chat_tools: ModelSpec,
}

impl Manifest {
    pub fn spec(&self, id: ModelId) -> &ModelSpec {
        match id {
            ModelId::Embed => &self.embed,
            ModelId::Chat => &self.chat,
            ModelId::ChatTools => &self.chat_tools,
        }
    }

    /// The pinned production models.
    pub fn pinned() -> Self {
        let f = FileSpec::new;
        Self {
            embed: ModelSpec {
                repo: "mlx-community/bge-m3-mlx-8bit".into(),
                commit: "7eca4a1c6ea1a0c5efc37598b369012f3985910f".into(),
                files: vec![
                    f(
                        "config.json",
                        814,
                        "4603d4b7f4ed1c9aa3590c279d0558d426264463403847d293f66ade9f23dc1b",
                    ),
                    f(
                        "model.safetensors",
                        603_620_090,
                        "57b597e5aa8c102c2698cc0915760a839e4c1a30bff0a46d7883d27d3f010720",
                    ),
                    f(
                        "model.safetensors.index.json",
                        51_249,
                        "40240743a3a26832574855f41cf9a1babbd5b7ffa71cc462aa058be26f931883",
                    ),
                    f(
                        "tokenizer.json",
                        17_098_085,
                        "5df1f55d60c9705a501ab9a75550728625740741fe4be308dac4806c16b7d51d",
                    ),
                    f(
                        "tokenizer_config.json",
                        379,
                        "c2ef99124628ae6f79a847ad67e5d3f5b016d0387de90c9fbaf17c45a98933af",
                    ),
                    f(
                        "special_tokens_map.json",
                        964,
                        "8c785abebea9ae3257b61681b4e6fd8365ceafde980c21970d001e834cf10835",
                    ),
                ],
            },
            chat: ModelSpec {
                repo: "mlx-community/Gemma4-E2B-IT-Text-int4".into(),
                commit: "61d85e83c959ac93109dc5be7104c8de9942ef66".into(),
                files: vec![
                    f(
                        "model.safetensors",
                        2_634_535_262,
                        "12fd23751e57dbe38fc9f69e2e7eea247e207b203ee27ea9fff0b1b56e1d621b",
                    ),
                    f(
                        "model.safetensors.index.json",
                        84_084,
                        "da9f33c151eff339ee0f66a2485e757af4aeb10ba3d551bf69bc8cad0110ae54",
                    ),
                    f(
                        "tokenizer.json",
                        36_459_693,
                        "12499b770c7ac2057affc48617f65ede6f1a7b849574967c177998691027afc6",
                    ),
                    f(
                        "tokenizer_config.json",
                        1_837,
                        "bbf66f6258a0e597b9f35b87db524a042bf208541731fcc6c4b60f19dc10c958",
                    ),
                    f(
                        "chat_template.jinja",
                        17_336,
                        "2f1b4d75d067bae3fe44e676721c7f077d243bc007156cb9c2f8b5836613d082",
                    ),
                    f(
                        "generation_config.json",
                        207,
                        "f59b6fa8fb6cf135f525fd203cd70c54016174fb0c3c78d1e40890f2f51395b3",
                    ),
                    FileSpec::bundled(
                        "config.json",
                        GEMMA_CONFIG,
                        "6cd31231eb175b938c42a4fa3dd78f4956b91756e7b81a9b1d9a2e4136e812e2",
                    ),
                ],
            },
            chat_tools: ModelSpec {
                repo: "mlx-community/gemma-4-e4b-it-4bit".into(),
                commit: "475b9088d29754a3379866cf5aeb6b41acd313c2".into(),
                files: vec![
                    f(
                        "model.safetensors",
                        5_146_800_534,
                        "932b8271fc3fe65adcc78b96c10c6268bbfb13e8f67d1358727c0d6ee97e1eff",
                    ),
                    f(
                        "model.safetensors.index.json",
                        240_961,
                        "f8accac59ee7efe87e0c298c854610b262c3cadd477407503147c71209ff0093",
                    ),
                    f(
                        "tokenizer.json",
                        32_169_626,
                        "cc8d3a0ce36466ccc1278bf987df5f71db1719b9ca6b4118264f45cb627bfe0f",
                    ),
                    f(
                        "tokenizer_config.json",
                        2_740,
                        "080d9e1aff284e2f6043889cd05367966f7c7b80e025fbc0b06745e218158656",
                    ),
                    f(
                        "chat_template.jinja",
                        17_336,
                        "2f1b4d75d067bae3fe44e676721c7f077d243bc007156cb9c2f8b5836613d082",
                    ),
                    f(
                        "generation_config.json",
                        208,
                        "d4226bbe3117d2d253ba4609720ba82c6c4ce4627a9a6ae05387c78983ac03de",
                    ),
                    f(
                        "processor_config.json",
                        1_316,
                        "de3e580aebdc98272d4c4547daffe6525fcbae18a83a0e0bcf0d7444d4ee6f37",
                    ),
                    f(
                        "config.json",
                        6_628,
                        "780ccb3a514a5f1ced161d383f948fc22eca9b84b752ca19494f625bd9bad7a6",
                    ),
                ],
            },
        }
    }
}
