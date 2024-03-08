use std::fs::File;

use mistralrs::{
    LlamaLoader as _LlamaLoader, LlamaSpecificConfig, Loader, MistralRs, ModelKind as _ModelKind,
    SchedulerMethod, TokenSource,
};
use pyo3::{exceptions::PyValueError, prelude::*};

use crate::{get_device, ModelKind, Runner};

#[pyclass]
/// A loader for a Runner.
pub struct LlamaLoader {
    loader: _LlamaLoader,
    no_kv_cache: bool,
}

#[pymethods]
impl LlamaLoader {
    #[new]
    #[pyo3(signature = (model_id, kind, no_kv_cache=false, use_flash_attn=cfg!(feature="flash-attn"), repeat_last_n=64, gqa=1, order_file=None, quantized_model_id=None,quantized_filename=None,xlora_model_id=None))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        model_id: String,
        kind: Py<ModelKind>,
        no_kv_cache: bool,
        mut use_flash_attn: bool,
        repeat_last_n: usize,
        gqa: usize,
        order_file: Option<String>,
        quantized_model_id: Option<String>,
        quantized_filename: Option<String>,
        xlora_model_id: Option<String>,
    ) -> PyResult<Self> {
        use_flash_attn &= cfg!(feature = "flash-attn");
        let order = if let Some(ref order_file) = order_file {
            let f = File::open(order_file);
            let f = match f {
                Ok(x) => x,
                Err(y) => return Err(PyValueError::new_err(y)),
            };
            match serde_json::from_reader(f) {
                Ok(x) => Some(x),
                Err(y) => return Err(PyValueError::new_err(y.to_string())),
            }
        } else {
            None
        };
        let kind = Python::with_gil(|py| match &*kind.as_ref(py).borrow() {
            ModelKind::Normal => _ModelKind::Normal,
            ModelKind::XLoraNormal => _ModelKind::XLoraNormal,
            ModelKind::QuantizedGGUF => _ModelKind::QuantizedGGUF,
            ModelKind::QuantizedGGML => _ModelKind::QuantizedGGML,
            ModelKind::XLoraGGUF => _ModelKind::XLoraGGUF,
            ModelKind::XLoraGGML => _ModelKind::XLoraGGML,
        });
        if matches!(kind, _ModelKind::Normal)
            && (order_file.is_some()
                || quantized_model_id.is_some()
                || quantized_filename.is_some()
                || xlora_model_id.is_some())
        {
            return Err(PyValueError::new_err("Expected no order file, no quantized model id, no quantized filename, and no xlora model id."));
        } else if matches!(kind, _ModelKind::XLoraNormal)
            && (order_file.is_none()
                || quantized_model_id.is_some()
                || quantized_filename.is_some()
                || xlora_model_id.is_none())
        {
            return Err(PyValueError::new_err("Expected an order file and xlora model id but no quantized model id and no quantized filename."));
        } else if matches!(kind, _ModelKind::QuantizedGGUF)
            || matches!(kind, _ModelKind::QuantizedGGML)
                && (order_file.is_some()
                    || quantized_model_id.is_none()
                    || quantized_filename.is_none()
                    || xlora_model_id.is_some())
        {
            return Err(PyValueError::new_err("Expected a quantized model id and quantized filename but no order file and no xlora model id."));
        } else if matches!(kind, _ModelKind::XLoraGGUF)
            || matches!(kind, _ModelKind::XLoraGGML)
                && (order_file.is_none()
                    || quantized_model_id.is_none()
                    || quantized_filename.is_none()
                    || xlora_model_id.is_none())
        {
            return Err(PyValueError::new_err("Expected a quantized model id and quantized filename and order file and xlora model id."));
        }
        Ok(Self {
            loader: _LlamaLoader::new(
                model_id,
                LlamaSpecificConfig {
                    use_flash_attn,
                    repeat_last_n,
                    gqa,
                },
                quantized_model_id,
                quantized_filename,
                xlora_model_id,
                kind,
                order,
                no_kv_cache,
            ),
            no_kv_cache,
        })
    }

    /// Specify token source and token source value as the following pairing:
    /// "cache" -> None
    /// "literal" -> str
    /// "envvar" -> str
    /// "path" -> str
    ///
    /// `log`:
    /// Log all responses and requests to this file
    ///
    /// `truncate_sequence`:
    /// If a sequence is larger than the maximum model length, truncate the number
    /// of tokens such that the sequence will fit at most the maximum length.
    /// If `max_tokens` is not specified in the request, space for 10 tokens will be reserved instead.
    ///
    /// `max_seqs`:
    /// Maximum running sequences at any time
    ///
    /// `no_kv_cache`:
    /// Use no KV cache.
    #[pyo3(signature = (token_source = "cache", max_seqs = 2, truncate_sequence = false, logfile = None, revision = None, token_source_value = None))]
    fn load(
        &mut self,
        token_source: &str,
        max_seqs: usize,
        truncate_sequence: bool,
        logfile: Option<String>,
        revision: Option<String>,
        token_source_value: Option<String>,
    ) -> PyResult<Runner> {
        let device = get_device();
        let device = match device {
            Ok(x) => x,
            Err(y) => return Err(PyValueError::new_err(y.to_string())),
        };

        let source = match (token_source, &token_source_value) {
            ("cache", None) => TokenSource::CacheToken,
            ("literal", Some(v)) => TokenSource::Literal(v.clone()),
            ("envvar", Some(env)) => TokenSource::EnvVar(env.clone()),
            ("path", Some(p)) => TokenSource::Path(p.clone()),
            _ => {
                return Err(PyValueError::new_err(format!(
                    "'{token_source}' and '{token_source_value:?}' are not compatible."
                )))
            }
        };

        let res = self.loader.load_model(revision, source, None, &device);
        let pipeline = match res {
            Ok(x) => x,
            Err(y) => return Err(PyValueError::new_err(y.to_string())),
        };

        let mistralrs = MistralRs::new(
            pipeline,
            SchedulerMethod::Fixed(max_seqs.try_into().unwrap()),
            logfile,
            truncate_sequence,
            self.no_kv_cache,
        );

        Ok(Runner { runner: mistralrs })
    }
}