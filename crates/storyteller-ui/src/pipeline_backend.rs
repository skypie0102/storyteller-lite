use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Instant, UNIX_EPOCH},
};
use storyteller_core::{
    prepare_job_sources, prepared_job_sources, run_cancellable_command,
    spawn_pipeline_worker_with_preflight, CommandOutput, CommandRunError, CommandStream,
    HardwareProfile, Job, JobWorkspace, LiveMetrics, PipelineBackend, PipelineEnvironment,
    PipelineStage, PipelineWorkerHandle, ResourceRequest, ResourceScheduler, RuntimeCoordinator,
    StagePlan, StageRunContext, StageRunError, StageRunOutput,
};

pub(crate) struct LitePipelineBackend {
    attempt_started: Instant,
    cpu_threads: usize,
}

impl LitePipelineBackend {
    pub(crate) fn new(cpu_threads: usize) -> Self {
        Self {
            attempt_started: Instant::now(),
            cpu_threads: cpu_threads.max(1),
        }
    }

    fn elapsed_millis(&self) -> u64 {
        self.attempt_started
            .elapsed()
            .as_millis()
            .min(u64::MAX as u128) as u64
    }

    fn run_prepare(
        &mut self,
        context: &mut StageRunContext<'_>,
    ) -> Result<StageRunOutput, StageRunError> {
        let stage_started = Instant::now();
        let activity_at = self.elapsed_millis();
        context
            .set_activity("Staging source EPUB and audiobook", activity_at)
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;
        let cancellation = context.cancellation_token();
        let prepared = match prepare_job_sources(context.job(), context.workspace(), &cancellation)
        {
            Ok(prepared) => prepared,
            Err(error) if cancellation.is_requested() => {
                return Err(StageRunError::cancelled(error, self.elapsed_millis()));
            }
            Err(error) => return Err(StageRunError::failed(error, self.elapsed_millis())),
        };

        let completed_at = self.elapsed_millis();
        context
            .set_stage_percent(100, completed_at)
            .map_err(|error| StageRunError::failed(error, completed_at))?;
        Ok(StageRunOutput::new(
            prepared.relative_artifacts(),
            stage_started.elapsed().as_secs(),
            completed_at,
        ))
    }

    fn run_analyze(
        &mut self,
        context: &mut StageRunContext<'_>,
    ) -> Result<StageRunOutput, StageRunError> {
        let stage_started = Instant::now();
        let prepared = prepared_job_sources(context.job(), context.workspace())
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;
        let runtime = AnalyzeRuntime::discover(context.job())
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;
        let stage_dir = context.workspace().stage_dir(PipelineStage::Analyze);
        reset_stage_dir(&stage_dir)
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;

        let wav_path = stage_dir.join("audio.wav");
        context
            .set_activity(
                "Converting audiobook to 16 kHz mono PCM",
                self.elapsed_millis(),
            )
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;
        let cancellation = context.cancellation_token();
        let mut ffmpeg = Command::new(&runtime.ffmpeg);
        ffmpeg
            .arg("-hide_banner")
            .arg("-nostdin")
            .arg("-y")
            .arg("-i")
            .arg(prepared.audiobook())
            .arg("-ar")
            .arg("16000")
            .arg("-ac")
            .arg("1")
            .arg("-c:a")
            .arg("pcm_s16le")
            .arg(&wav_path);
        let conversion = run_cancellable_command(&mut ffmpeg, &cancellation, |_, _| {});
        match conversion {
            Ok(output) if output.success => {}
            Ok(output) => {
                return Err(StageRunError::failed(
                    command_failure("ffmpeg audio conversion", &output),
                    self.elapsed_millis(),
                ));
            }
            Err(CommandRunError::Cancelled) => {
                return Err(StageRunError::cancelled(
                    "Audio conversion was cancelled.",
                    self.elapsed_millis(),
                ));
            }
            Err(error) => {
                return Err(StageRunError::failed(
                    format!("Could not run ffmpeg audio conversion: {error}"),
                    self.elapsed_millis(),
                ));
            }
        }
        validate_nonempty_file(&wav_path, "Converted transcription audio")
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;

        let mut metrics = LiveMetrics {
            backend: Some("whisper.cpp CLI".into()),
            model: Some(runtime.model_name.clone()),
            ..LiveMetrics::default()
        };
        context.set_metrics(metrics.clone(), self.elapsed_millis());
        context
            .set_activity("Transcribing audiobook with whisper.cpp", self.elapsed_millis())
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;

        let output_prefix = stage_dir.join("transcript");
        let transcript_path = output_prefix.with_extension("json");
        let mut whisper = Command::new(&runtime.whisper_cli);
        whisper
            .arg("-m")
            .arg(&runtime.whisper_model)
            .arg("-f")
            .arg(&wav_path)
            .arg("-l")
            .arg(&runtime.language)
            .arg("-ojf")
            .arg("-of")
            .arg(&output_prefix)
            .arg("-pp")
            .arg("-t")
            .arg(self.cpu_threads.to_string());

        let mut progress_error = None;
        let transcription = run_cancellable_command(&mut whisper, &cancellation, |stream, line| {
            if stream != CommandStream::Stderr {
                return;
            }
            if let Some(percent) = parse_whisper_progress(line) {
                if let Err(error) = context.set_stage_percent(percent, 0) {
                    if progress_error.is_none() {
                        progress_error = Some(error);
                    }
                }
            }
            if let Some(backend) = parse_whisper_backend(line) {
                metrics.backend = Some(backend);
                context.set_metrics(metrics.clone(), 0);
            }
        });
        if let Some(error) = progress_error {
            return Err(StageRunError::failed(error, self.elapsed_millis()));
        }
        match transcription {
            Ok(output) if output.success => {}
            Ok(output) => {
                return Err(StageRunError::failed(
                    command_failure("whisper.cpp transcription", &output),
                    self.elapsed_millis(),
                ));
            }
            Err(CommandRunError::Cancelled) => {
                return Err(StageRunError::cancelled(
                    "Transcription was cancelled.",
                    self.elapsed_millis(),
                ));
            }
            Err(error) => {
                return Err(StageRunError::failed(
                    format!("Could not run whisper.cpp transcription: {error}"),
                    self.elapsed_millis(),
                ));
            }
        }
        validate_nonempty_file(&transcript_path, "Whisper transcript")
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;

        Ok(StageRunOutput::new(
            vec![PathBuf::from("audio.wav"), PathBuf::from("transcript.json")],
            stage_started.elapsed().as_secs(),
            self.elapsed_millis(),
        ))
    }
}

impl PipelineBackend for LitePipelineBackend {
    fn plan_stage(&mut self, _job: &Job, stage: PipelineStage) -> Result<StagePlan, String> {
        match stage {
            PipelineStage::Prepare => Ok(StagePlan::run(
                "Preparing source files",
                ResourceRequest::io_heavy(1),
                self.elapsed_millis(),
            )),
            PipelineStage::Analyze => Ok(StagePlan::run(
                "Analyzing audiobook",
                ResourceRequest::cpu_heavy(self.cpu_threads),
                self.elapsed_millis(),
            )),
            _ => Err(format!(
                "{} backend is not implemented in Storyteller Lite yet.",
                stage.label()
            )),
        }
    }

    fn run_stage(
        &mut self,
        context: &mut StageRunContext<'_>,
    ) -> Result<StageRunOutput, StageRunError> {
        match context.stage() {
            PipelineStage::Prepare => self.run_prepare(context),
            PipelineStage::Analyze => self.run_analyze(context),
            stage => Err(StageRunError::failed(
                format!(
                    "{} backend is not implemented in Storyteller Lite yet.",
                    stage.label()
                ),
                self.elapsed_millis(),
            )),
        }
    }
}

#[derive(Debug, Clone)]
struct AnalyzeRuntime {
    ffmpeg: PathBuf,
    whisper_cli: PathBuf,
    whisper_model: PathBuf,
    model_name: String,
    language: String,
}

impl AnalyzeRuntime {
    fn discover(job: &Job) -> Result<Self, String> {
        let model_name = job.settings.whisper_model.trim().to_string();
        if model_name.is_empty() {
            return Err("Whisper model name cannot be blank.".into());
        }
        Ok(Self {
            ffmpeg: resolve_executable("STORYTELLER_FFMPEG", "ffmpeg"),
            whisper_cli: resolve_executable("STORYTELLER_WHISPER", "whisper-cli"),
            whisper_model: resolve_whisper_model(&model_name)?,
            model_name,
            language: effective_language(job),
        })
    }
}

pub(crate) fn spawn_job_worker(job: Job) -> Result<PipelineWorkerHandle, String> {
    let logical_cpu_threads = std::thread::available_parallelism()
        .map(|threads| threads.get())
        .unwrap_or(1);
    let scheduler = ResourceScheduler::automatic(&HardwareProfile {
        logical_cpu_threads,
        memory_gib: None,
        gpu_backend: None,
        gpu_vram_mib: None,
    })?;
    let mut runtime = RuntimeCoordinator::new(scheduler);
    runtime.register_job(job.id)?;
    let workspace = JobWorkspace::for_job(workspace_base(), job.id);
    let environment = pipeline_environment(&job);

    spawn_pipeline_worker_with_preflight(
        job,
        workspace,
        runtime,
        environment,
        LitePipelineBackend::new(logical_cpu_threads),
    )
}

fn pipeline_environment(job: &Job) -> PipelineEnvironment {
    let whisper_cli = resolve_executable("STORYTELLER_WHISPER", "whisper-cli");
    let model_name = job.settings.whisper_model.trim();
    let effective_whisper_model = if model_name.is_empty() {
        String::new()
    } else {
        match resolve_whisper_model(model_name) {
            Ok(path) => file_identity("whisper-model", &path),
            Err(_) => format!("requested:{model_name}"),
        }
    };

    PipelineEnvironment {
        whisper_backend: file_identity("whisper.cpp-cli", &whisper_cli),
        alignment_backend: "unimplemented:alignment".into(),
        ocr_backend: "unimplemented:ocr".into(),
        epub_backend: "unimplemented:epub".into(),
        effective_language: effective_language(job),
        effective_whisper_model,
    }
}

fn effective_language(job: &Job) -> String {
    job.settings
        .language
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("auto")
        .to_string()
}

fn resolve_executable(environment_variable: &str, base_name: &str) -> PathBuf {
    if let Some(value) = env::var_os(environment_variable).filter(|value| !value.is_empty()) {
        return PathBuf::from(value);
    }

    let file_name = executable_file_name(base_name);
    if let Some(executable_dir) = current_executable_dir() {
        for candidate in [
            executable_dir.join("tools").join(&file_name),
            executable_dir.join(&file_name),
        ] {
            if candidate.is_file() {
                return candidate;
            }
        }
    }

    PathBuf::from(base_name)
}

fn resolve_whisper_model(model_name: &str) -> Result<PathBuf, String> {
    if let Some(value) = env::var_os("STORYTELLER_WHISPER_MODEL").filter(|value| !value.is_empty())
    {
        let path = PathBuf::from(value);
        validate_nonempty_file(&path, "Configured Whisper model")?;
        return Ok(path);
    }

    let file_name = format!("ggml-{model_name}.bin");
    let mut candidates = Vec::new();
    if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
        candidates.push(
            PathBuf::from(local_app_data)
                .join("Storyteller OneClick Lite")
                .join("models")
                .join(&file_name),
        );
    }
    if let Some(executable_dir) = current_executable_dir() {
        candidates.push(executable_dir.join("models").join(&file_name));
        candidates.push(executable_dir.join("tools").join("models").join(&file_name));
    }
    if let Ok(current_dir) = env::current_dir() {
        candidates.push(current_dir.join("models").join(&file_name));
    }

    if let Some(path) = candidates.into_iter().find(|candidate| candidate.is_file()) {
        validate_nonempty_file(&path, "Whisper model")?;
        return Ok(path);
    }

    Err(format!(
        "Whisper model {file_name} was not found. Place it in the app models folder or set STORYTELLER_WHISPER_MODEL."
    ))
}

fn current_executable_dir() -> Option<PathBuf> {
    env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
}

fn executable_file_name(base_name: &str) -> String {
    if cfg!(windows) {
        format!("{base_name}.exe")
    } else {
        base_name.to_string()
    }
}

fn file_identity(label: &str, path: &Path) -> String {
    let Ok(metadata) = fs::metadata(path) else {
        return format!("{label}:{}", path.display());
    };
    if !metadata.is_file() {
        return format!("{label}:{}", path.display());
    }
    let modified = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_secs())
        .unwrap_or(0);
    format!("{label}:{}:{}:{modified}", path.display(), metadata.len())
}

fn reset_stage_dir(stage_dir: &Path) -> Result<(), String> {
    if stage_dir.exists() {
        fs::remove_dir_all(stage_dir).map_err(|error| {
            format!(
                "Could not reset Analyze workspace {}: {error}",
                stage_dir.display()
            )
        })?;
    }
    fs::create_dir_all(stage_dir).map_err(|error| {
        format!(
            "Could not create Analyze workspace {}: {error}",
            stage_dir.display()
        )
    })
}

fn validate_nonempty_file(path: &Path, label: &str) -> Result<(), String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("{label} is unavailable at {}: {error}", path.display()))?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(format!(
            "{label} is not a non-empty regular file: {}",
            path.display()
        ));
    }
    Ok(())
}

fn parse_whisper_progress(line: &str) -> Option<u8> {
    let (_, remainder) = line.split_once("progress =")?;
    let (percent, _) = remainder.split_once('%')?;
    percent.trim().parse::<u8>().ok().filter(|value| *value <= 100)
}

fn parse_whisper_backend(line: &str) -> Option<String> {
    let marker = "backend_init_gpu: using ";
    let (_, backend) = line.split_once(marker)?;
    let backend = backend.trim();
    if backend.is_empty() {
        None
    } else {
        Some(format!("whisper.cpp / {backend}"))
    }
}

fn command_failure(label: &str, output: &CommandOutput) -> String {
    let diagnostics = diagnostic_tail(if output.stderr.trim().is_empty() {
        &output.stdout
    } else {
        &output.stderr
    });
    let exit = output
        .exit_code
        .map(|code| code.to_string())
        .unwrap_or_else(|| "unknown".into());
    if diagnostics.is_empty() {
        format!("{label} failed with exit code {exit}.")
    } else {
        format!("{label} failed with exit code {exit}: {diagnostics}")
    }
}

fn diagnostic_tail(text: &str) -> String {
    let mut lines = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .rev()
        .take(8)
        .collect::<Vec<_>>();
    lines.reverse();
    lines.join(" | ")
}

fn workspace_base() -> PathBuf {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir)
        .join("Storyteller OneClick Lite")
        .join("jobs")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whisper_progress_parser_accepts_real_cli_shape() {
        assert_eq!(
            parse_whisper_progress("whisper_print_progress_callback: progress =  42%"),
            Some(42)
        );
        assert_eq!(parse_whisper_progress("not progress"), None);
    }

    #[test]
    fn whisper_backend_parser_only_reports_observed_backend() {
        assert_eq!(
            parse_whisper_backend("whisper_backend_init_gpu: using CUDA0 backend"),
            Some("whisper.cpp / CUDA0 backend".into())
        );
        assert_eq!(parse_whisper_backend("system_info: CPU only"), None);
    }
}
