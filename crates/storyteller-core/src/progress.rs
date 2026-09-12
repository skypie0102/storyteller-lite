#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PipelineStage {
    Prepare,
    Analyze,
    Align,
    ReviewAudio,
    Encode,
    BuildEpub,
    Validate,
}

impl PipelineStage {
    pub const ALL: [Self; 7] = [
        Self::Prepare,
        Self::Analyze,
        Self::Align,
        Self::ReviewAudio,
        Self::Encode,
        Self::BuildEpub,
        Self::Validate,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Prepare => "Prepare",
            Self::Analyze => "Analyze",
            Self::Align => "Align",
            Self::ReviewAudio => "Review Audio",
            Self::Encode => "Encode",
            Self::BuildEpub => "Build EPUB",
            Self::Validate => "Validate",
        }
    }

    pub const fn index(self) -> usize {
        match self {
            Self::Prepare => 0,
            Self::Analyze => 1,
            Self::Align => 2,
            Self::ReviewAudio => 3,
            Self::Encode => 4,
            Self::BuildEpub => 5,
            Self::Validate => 6,
        }
    }

    const fn weight(self) -> u16 {
        match self {
            Self::Prepare => 5,
            Self::Analyze => 35,
            Self::Align => 25,
            Self::ReviewAudio => 10,
            Self::Encode => 8,
            Self::BuildEpub => 12,
            Self::Validate => 5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageStatus {
    Pending,
    Running,
    Completed,
    Cached,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageProgress {
    pub stage: PipelineStage,
    pub status: StageStatus,
    pub percent: u8,
    pub elapsed_seconds: Option<u64>,
}

impl StageProgress {
    fn pending(stage: PipelineStage) -> Self {
        Self {
            stage,
            status: StageStatus::Pending,
            percent: 0,
            elapsed_seconds: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LiveMetrics {
    pub processed_audio_seconds: Option<f64>,
    pub total_audio_seconds: Option<f64>,
    pub current_item: Option<u64>,
    pub total_items: Option<u64>,
    pub speed_factor: Option<f64>,
    pub eta_seconds: Option<u64>,
    pub match_percent: Option<f64>,
    pub backend: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PipelineProgress {
    stages: Vec<StageProgress>,
    current_activity: String,
    metrics: LiveMetrics,
}

impl Default for PipelineProgress {
    fn default() -> Self {
        Self {
            stages: PipelineStage::ALL
                .into_iter()
                .map(StageProgress::pending)
                .collect(),
            current_activity: "Waiting".into(),
            metrics: LiveMetrics::default(),
        }
    }
}

impl PipelineProgress {
    pub fn stages(&self) -> &[StageProgress] {
        &self.stages
    }

    pub fn current_stage(&self) -> Option<PipelineStage> {
        self.stages
            .iter()
            .find(|stage| stage.status == StageStatus::Running)
            .map(|stage| stage.stage)
    }

    pub fn current_stage_percent(&self) -> Option<u8> {
        self.stages
            .iter()
            .find(|stage| stage.status == StageStatus::Running)
            .map(|stage| stage.percent)
    }

    pub fn current_activity(&self) -> &str {
        &self.current_activity
    }

    pub fn metrics(&self) -> &LiveMetrics {
        &self.metrics
    }

    pub fn set_metrics(&mut self, metrics: LiveMetrics) {
        self.metrics = metrics;
    }

    pub fn set_activity(&mut self, activity: impl Into<String>) -> Result<(), String> {
        let activity = activity.into();
        if activity.trim().is_empty() {
            return Err("Pipeline activity cannot be blank.".into());
        }
        self.current_activity = activity;
        Ok(())
    }

    pub fn start_stage(
        &mut self,
        stage: PipelineStage,
        activity: impl Into<String>,
    ) -> Result<(), String> {
        if let Some(active) = self.current_stage() {
            return Err(format!(
                "Cannot start {} while {} is active.",
                stage.label(),
                active.label()
            ));
        }
        let item = &mut self.stages[stage.index()];
        if !matches!(item.status, StageStatus::Pending | StageStatus::Failed) {
            return Err(format!("{} is not pending.", stage.label()));
        }
        item.status = StageStatus::Running;
        item.percent = 0;
        item.elapsed_seconds = None;
        self.set_activity(activity)
    }

    pub fn set_current_stage_percent(&mut self, percent: u8) -> Result<(), String> {
        if percent > 100 {
            return Err("Stage progress cannot exceed 100%.".into());
        }
        let Some(stage) = self
            .stages
            .iter_mut()
            .find(|stage| stage.status == StageStatus::Running)
        else {
            return Err("No pipeline stage is active.".into());
        };
        stage.percent = percent;
        Ok(())
    }

    pub fn complete_stage(
        &mut self,
        stage: PipelineStage,
        elapsed_seconds: u64,
    ) -> Result<(), String> {
        let item = &mut self.stages[stage.index()];
        if item.status != StageStatus::Running {
            return Err(format!("{} is not active.", stage.label()));
        }
        item.status = StageStatus::Completed;
        item.percent = 100;
        item.elapsed_seconds = Some(elapsed_seconds);
        Ok(())
    }

    pub fn mark_cached(
        &mut self,
        stage: PipelineStage,
        elapsed_seconds: Option<u64>,
    ) -> Result<(), String> {
        let item = &mut self.stages[stage.index()];
        if item.status == StageStatus::Running {
            return Err(format!("Cannot cache active stage {}.", stage.label()));
        }
        item.status = StageStatus::Cached;
        item.percent = 100;
        item.elapsed_seconds = elapsed_seconds;
        Ok(())
    }

    pub fn mark_failed(&mut self, stage: PipelineStage) -> Result<(), String> {
        let item = &mut self.stages[stage.index()];
        if item.status != StageStatus::Running {
            return Err(format!("{} is not active.", stage.label()));
        }
        item.status = StageStatus::Failed;
        Ok(())
    }

    pub fn reset_stage(&mut self, stage: PipelineStage) {
        self.stages[stage.index()] = StageProgress::pending(stage);
    }

    pub fn reset_from(&mut self, stage: PipelineStage) {
        for current in PipelineStage::ALL.into_iter().skip(stage.index()) {
            self.reset_stage(current);
        }
        self.metrics = LiveMetrics::default();
        self.current_activity = "Waiting".into();
    }

    pub fn overall_percent(&self) -> u8 {
        let weighted = self
            .stages
            .iter()
            .map(|stage| {
                let completion = match stage.status {
                    StageStatus::Completed | StageStatus::Cached | StageStatus::Skipped => 100,
                    StageStatus::Running | StageStatus::Failed => stage.percent as u16,
                    StageStatus::Pending => 0,
                };
                stage.stage.weight() * completion
            })
            .sum::<u16>();
        (weighted / 100).min(100) as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stages_have_stable_user_facing_order() {
        assert_eq!(
            PipelineStage::ALL.map(PipelineStage::label),
            [
                "Prepare",
                "Analyze",
                "Align",
                "Review Audio",
                "Encode",
                "Build EPUB",
                "Validate"
            ]
        );
    }

    #[test]
    fn overall_progress_is_weighted_without_fractional_claims() {
        let mut progress = PipelineProgress::default();
        progress.start_stage(PipelineStage::Prepare, "Preparing").unwrap();
        progress.set_current_stage_percent(50).unwrap();
        assert_eq!(progress.overall_percent(), 2);
        progress.complete_stage(PipelineStage::Prepare, 1).unwrap();
        assert_eq!(progress.overall_percent(), 5);
    }
}
