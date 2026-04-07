use std::fs::File;
use std::path::{Path, PathBuf};

use half::f16;
use memmap2::Mmap;
use safetensors::SafeTensors;
use tokenizers::Tokenizer;

use crate::embed::error::EmbedError;

struct MmapF32 {
    _mmap: Mmap,
    data_ptr: *const f32,
    rows: usize,
    cols: usize,
}

#[expect(
    unsafe_code,
    reason = "MmapF32 holds immutable mmap + derived pointer, safe to send"
)]
unsafe impl Send for MmapF32 {}
#[expect(
    unsafe_code,
    reason = "MmapF32 holds immutable mmap + derived pointer, safe to share"
)]
unsafe impl Sync for MmapF32 {}

impl MmapF32 {
    fn row(&self, idx: usize) -> &[f32] {
        assert!(
            idx < self.rows,
            "row index {idx} out of bounds (rows={})",
            self.rows
        );
        let start = idx * self.cols;
        #[expect(unsafe_code, reason = "zero-copy row access into mmap-backed f32 data")]
        unsafe {
            std::slice::from_raw_parts(self.data_ptr.add(start), self.cols)
        }
    }
}

pub struct MmapStaticModel {
    tokenizer: Tokenizer,
    mmap_f32: Option<MmapF32>,
    owned_f32: Option<Vec<f32>>,
    cols: usize,
    weights: Option<Vec<f32>>,
    token_mapping: Option<Vec<usize>>,
    normalize: bool,
    median_token_length: usize,
    unk_token_id: Option<usize>,
    model_name: String,
}

impl MmapStaticModel {
    pub fn from_local(model_dir: &Path, model_name: &str) -> Result<Self, EmbedError> {
        let tok_path = model_dir.join("tokenizer.json");
        let mdl_path = model_dir.join("model.safetensors");
        let cfg_path = model_dir.join("config.json");
        for p in [&tok_path, &mdl_path, &cfg_path] {
            if !p.exists() {
                return Err(EmbedError::ModelInit(format!(
                    "Missing file: {}",
                    p.display()
                )));
            }
        }
        let tok_handle = std::thread::spawn(move || load_tokenizer_cached(&tok_path));
        let cfg_file = File::open(&cfg_path)
            .map_err(|e| EmbedError::ModelInit(format!("config.json: {e}")))?;
        let cfg: serde_json::Value = serde_json::from_reader(&cfg_file)
            .map_err(|e| EmbedError::ModelInit(format!("config.json parse: {e}")))?;
        let normalize = cfg
            .get("normalize")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let file = File::open(&mdl_path)
            .map_err(|e| EmbedError::ModelInit(format!("open safetensors: {e}")))?;
        #[expect(
            unsafe_code,
            reason = "mmap requires unsafe; file is read-only, lifetime managed by struct"
        )]
        let mmap =
            unsafe { Mmap::map(&file) }.map_err(|e| EmbedError::ModelInit(format!("mmap: {e}")))?;
        let safet = SafeTensors::deserialize(&mmap)
            .map_err(|e| EmbedError::ModelInit(format!("safetensors: {e}")))?;
        let tensor = safet
            .tensor("embeddings")
            .or_else(|_| safet.tensor("0"))
            .map_err(|e| EmbedError::ModelInit(format!("embeddings tensor: {e}")))?;
        let shape: [usize; 2] = tensor
            .shape()
            .try_into()
            .map_err(|_| EmbedError::ModelInit("embedding tensor is not 2-D".into()))?;
        let [rows, cols] = shape;
        let dtype = tensor.dtype();
        let weights = load_optional_f32_tensor(&safet, "weights");
        let token_mapping = load_optional_mapping(&safet);
        let data_bytes = tensor.data();
        let data_offset = data_bytes.as_ptr() as usize - mmap.as_ptr() as usize;
        let (mmap_f32, owned_f32) = match dtype {
            safetensors::Dtype::F32 => {
                let ptr = mmap.as_ptr().wrapping_add(data_offset);
                if ptr.align_offset(std::mem::align_of::<f32>()) == 0 {
                    eprintln!(
                        "  [mmap] f32 zero-copy: {rows}x{cols} ({} MB virtual)",
                        rows * cols * 4 / 1_048_576
                    );
                    let data_ptr = ptr.cast::<f32>();
                    let mf32 = MmapF32 {
                        _mmap: mmap,
                        data_ptr,
                        rows,
                        cols,
                    };
                    (Some(mf32), None)
                } else {
                    eprintln!("  [mmap] f32 data misaligned, falling back to copy");
                    let floats: Vec<f32> = data_bytes
                        .chunks_exact(4)
                        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                        .collect();
                    drop(mmap);
                    (None, Some(floats))
                }
            }
            safetensors::Dtype::F16 => {
                eprintln!("  [mmap] f16 model — converting to f32 (mmap avoids double alloc)");
                let floats: Vec<f32> = data_bytes
                    .chunks_exact(2)
                    .map(|b| f16::from_le_bytes([b[0], b[1]]).to_f32())
                    .collect();
                drop(mmap);
                (None, Some(floats))
            }
            safetensors::Dtype::I8 => {
                eprintln!("  [mmap] i8 model — converting to f32");
                let floats: Vec<f32> = data_bytes.iter().map(|&b| f32::from(b as i8)).collect();
                drop(mmap);
                (None, Some(floats))
            }
            other => {
                return Err(EmbedError::ModelInit(format!(
                    "unsupported dtype: {other:?}"
                )));
            }
        };
        let tokenizer = tok_handle
            .join()
            .map_err(|_| EmbedError::ModelInit("tokenizer thread panicked".into()))??;
        let vocab_size = tokenizer.get_vocab_size(false);
        let median_token_length = if vocab_size > 100_000 { 6 } else { 4 };
        let unk_token_id = tokenizer
            .token_to_id("[UNK]")
            .or_else(|| tokenizer.token_to_id("<unk>"))
            .map(|id| id as usize);
        Ok(Self {
            tokenizer,
            mmap_f32,
            owned_f32,
            cols,
            weights,
            token_mapping,
            normalize,
            median_token_length,
            unk_token_id,
            model_name: model_name.to_string(),
        })
    }
    pub fn from_pretrained(model_name: &str) -> Result<Self, EmbedError> {
        let model_dir = resolve_model_path(model_name)?;
        Self::from_local(&model_dir, model_name)
    }
    pub fn dim(&self) -> usize {
        self.cols
    }
    pub fn name(&self) -> &str {
        &self.model_name
    }
    fn get_row(&self, idx: usize) -> &[f32] {
        if let Some(ref mf) = self.mmap_f32 {
            mf.row(idx)
        } else if let Some(ref data) = self.owned_f32 {
            let start = idx * self.cols;
            &data[start..start + self.cols]
        } else {
            unreachable!("no embedding data")
        }
    }
    pub fn encode_single(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        let truncated = truncate_str(text, 512, self.median_token_length);
        let encoding = self
            .tokenizer
            .encode(truncated, false)
            .map_err(|e| EmbedError::EmbeddingFailed(format!("tokenization: {e}")))?;
        let mut ids = encoding.get_ids().to_vec();
        if let Some(unk) = self.unk_token_id {
            ids.retain(|&id| id as usize != unk);
        }
        ids.truncate(512);
        Ok(self.pool_ids(&ids))
    }
    pub fn encode_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        let truncated: Vec<&str> = texts
            .iter()
            .map(|t| truncate_str(t, 512, self.median_token_length))
            .collect();
        let encodings = self
            .tokenizer
            .encode_batch_fast::<String>(truncated.into_iter().map(Into::into).collect(), false)
            .map_err(|e| EmbedError::EmbeddingFailed(format!("batch tokenization: {e}")))?;
        Ok(encodings
            .into_iter()
            .map(|enc| {
                let mut ids = enc.get_ids().to_vec();
                if let Some(unk) = self.unk_token_id {
                    ids.retain(|&id| id as usize != unk);
                }
                ids.truncate(512);
                self.pool_ids(&ids)
            })
            .collect())
    }
    fn pool_ids(&self, ids: &[u32]) -> Vec<f32> {
        let dim = self.cols;
        let mut sum = vec![0.0f32; dim];
        let mut cnt = 0usize;
        for &id in ids {
            let tok = id as usize;
            let row_idx = self
                .token_mapping
                .as_ref()
                .and_then(|m| m.get(tok).copied())
                .unwrap_or(tok);
            let scale = self
                .weights
                .as_ref()
                .and_then(|w| w.get(tok).copied())
                .unwrap_or(1.0);
            let row = self.get_row(row_idx);
            for (i, &v) in row.iter().enumerate() {
                sum[i] += v * scale;
            }
            cnt += 1;
        }
        let denom = cnt.max(1) as f32;
        for x in &mut sum {
            *x /= denom;
        }
        if self.normalize {
            let norm = sum.iter().map(|&v| v * v).sum::<f32>().sqrt().max(1e-12);
            for x in &mut sum {
                *x /= norm;
            }
        }
        sum
    }
}

fn load_tokenizer_cached(tok_path: &Path) -> Result<Tokenizer, EmbedError> {
    let file =
        File::open(tok_path).map_err(|e| EmbedError::ModelInit(format!("tokenizer open: {e}")))?;
    #[expect(unsafe_code, reason = "mmap for read-only tokenizer JSON")]
    let mmap = unsafe { Mmap::map(&file) }
        .map_err(|e| EmbedError::ModelInit(format!("tokenizer mmap: {e}")))?;
    Tokenizer::from_bytes(&mmap).map_err(|e| EmbedError::ModelInit(format!("tokenizer parse: {e}")))
}

fn truncate_str(s: &str, max_tokens: usize, median_len: usize) -> &str {
    let max_chars = max_tokens.saturating_mul(median_len);
    match s.char_indices().nth(max_chars) {
        Some((byte_idx, _)) => &s[..byte_idx],
        None => s,
    }
}

fn load_optional_f32_tensor(safet: &SafeTensors<'_>, name: &str) -> Option<Vec<f32>> {
    let t = safet.tensor(name).ok()?;
    let raw = t.data();
    let floats = match t.dtype() {
        safetensors::Dtype::F64 => raw
            .chunks_exact(8)
            .filter_map(|b| b.try_into().ok().map(|a| f64::from_le_bytes(a) as f32))
            .collect(),
        safetensors::Dtype::F32 => raw
            .chunks_exact(4)
            .filter_map(|b| b.try_into().ok().map(f32::from_le_bytes))
            .collect(),
        safetensors::Dtype::F16 => raw
            .chunks_exact(2)
            .filter_map(|b| b.try_into().ok().map(|a| f16::from_le_bytes(a).to_f32()))
            .collect(),
        _ => return None,
    };
    Some(floats)
}

fn load_optional_mapping(safet: &SafeTensors<'_>) -> Option<Vec<usize>> {
    let t = safet.tensor("mapping").ok()?;
    let raw = t.data();
    let mapping = raw
        .chunks_exact(4)
        .filter_map(|b| b.try_into().ok().map(|a| i32::from_le_bytes(a) as usize))
        .collect();
    Some(mapping)
}

fn resolve_model_path(model_name: &str) -> Result<PathBuf, EmbedError> {
    let local = Path::new(model_name);
    if local.exists() && local.join("model.safetensors").exists() {
        return Ok(local.to_path_buf());
    }
    let api = hf_hub::api::sync::Api::new()
        .map_err(|e| EmbedError::Download(format!("HF API init: {e}")))?;
    let repo = api.model(model_name.to_string());
    let _tok = repo
        .get("tokenizer.json")
        .map_err(|e| EmbedError::Download(format!("tokenizer.json: {e}")))?;
    let mdl = repo
        .get("model.safetensors")
        .map_err(|e| EmbedError::Download(format!("model.safetensors: {e}")))?;
    let _cfg = repo
        .get("config.json")
        .map_err(|e| EmbedError::Download(format!("config.json: {e}")))?;
    mdl.parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| EmbedError::Download("safetensors file has no parent directory".into()))
}
