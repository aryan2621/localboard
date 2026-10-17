use std::path::PathBuf;

use serde::Serialize;

/// An open GGUF model the bundled llama.cpp runs with native tool calling (4-bit, Unsloth builds).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSpec {
    pub id: &'static str,
    pub name: &'static str,
    #[serde(skip)]
    pub repo: &'static str,
    #[serde(skip)]
    pub file: &'static str,
    pub size_mb: u64,
    pub min_ram_gb: u64,
    pub note: &'static str,
}

/// Small embedding model. Kept out of [`CATALOG`] so it is never chosen as a chat model.
pub const EMBEDDING: ModelSpec = ModelSpec {
    id: "nomic-embed-text-v1.5",
    name: "Nomic Embed Text v1.5",
    repo: "nomic-ai/nomic-embed-text-v1.5-GGUF",
    file: "nomic-embed-text-v1.5.Q8_0.gguf",
    size_mb: 146,
    min_ram_gb: 2,
    note: "Finds the right analyst, examples and notes. About 150 MB. Without it, matching uses keywords.",
};

/// Ordered weakest to strongest.
pub const CATALOG: [ModelSpec; 5] = [
    ModelSpec {
        id: "qwen3.5-4b",
        name: "Qwen3.5 4B",
        repo: "unsloth/Qwen3.5-4B-GGUF",
        file: "Qwen3.5-4B-Q4_K_M.gguf",
        size_mb: 2740,
        min_ram_gb: 8,
        note: "Small and quick. Good for simple lookups on any machine.",
    },
    ModelSpec {
        id: "qwen3.5-9b",
        name: "Qwen3.5 9B",
        repo: "unsloth/Qwen3.5-9B-GGUF",
        file: "Qwen3.5-9B-Q4_K_M.gguf",
        size_mb: 5680,
        min_ram_gb: 16,
        note: "The best balance of speed and accuracy for SQL and multi-step analysis.",
    },
    ModelSpec {
        id: "gemma-4-12b",
        name: "Gemma 4 12B",
        repo: "unsloth/gemma-4-12B-it-qat-GGUF",
        file: "gemma-4-12B-it-qat-UD-Q4_K_XL.gguf",
        size_mb: 6720,
        min_ram_gb: 16,
        note: "Google's model. A good second opinion next to Qwen.",
    },
    ModelSpec {
        id: "gemma-4-26b-a4b",
        name: "Gemma 4 26B A4B",
        repo: "unsloth/gemma-4-26B-A4B-it-qat-GGUF",
        file: "gemma-4-26B-A4B-it-qat-UD-Q4_K_XL.gguf",
        size_mb: 14250,
        min_ram_gb: 32,
        note: "Large but fast: only 4B of its parameters work on each word.",
    },
    ModelSpec {
        id: "qwen3.8-27b",
        name: "Qwen3.8 27B",
        repo: "unsloth/Qwen3.8-27B-GGUF",
        file: "Qwen3.8-27B-UD-Q4_K_M.gguf",
        size_mb: 16460,
        min_ram_gb: 32,
        note: "The most capable model here, close to cloud models. Slower.",
    },
];

pub fn spec(id: &str) -> Option<&'static ModelSpec> {
    CATALOG.iter().find(|model| model.id == id)
}

/// A chat model or the embedding model.
pub fn find(id: &str) -> Option<&'static ModelSpec> {
    spec(id).or((id == EMBEDDING.id).then_some(&EMBEDDING))
}

/// The strongest model this machine runs comfortably.
pub fn recommended(ram_gb: u64) -> &'static ModelSpec {
    ["qwen3.8-27b", "qwen3.5-9b"]
        .iter()
        .filter_map(|id| spec(id))
        .find(|model| model.min_ram_gb <= ram_gb)
        .unwrap_or(&CATALOG[0])
}

/// Where GGUF files live: LocalBoard's own models folder, and nowhere else.
pub struct ModelStore {
    dir: PathBuf,
}

impl ModelStore {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn own_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.gguf"))
    }

    pub fn path(&self, id: &str) -> Option<PathBuf> {
        Some(self.own_path(id)).filter(|path| path.is_file())
    }

    pub fn installed(&self) -> Vec<&'static ModelSpec> {
        CATALOG.iter().filter(|model| self.path(model.id).is_some()).collect()
    }

    /// The chosen model if installed, else the recommended one, else the strongest that fits.
    pub fn resolve(&self, requested: Option<&str>, ram_gb: u64) -> Option<&'static ModelSpec> {
        if let Some(model) = requested.and_then(spec).filter(|model| self.path(model.id).is_some()) {
            return Some(model);
        }
        let installed = self.installed();
        let best = recommended(ram_gb);
        if installed.iter().any(|model| model.id == best.id) {
            return Some(best);
        }
        let fitting: Vec<_> = installed.iter().copied().filter(|model| model.min_ram_gb <= ram_gb).collect();
        fitting.last().or(installed.last()).copied()
    }
}
