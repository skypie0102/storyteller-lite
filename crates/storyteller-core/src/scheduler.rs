use crate::JobId;
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HardwareProfile {
    pub logical_cpu_threads: usize,
    pub memory_gib: Option<u64>,
    pub gpu_backend: Option<String>,
    pub gpu_vram_mib: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceRequest {
    cpu_threads: usize,
    io_slots: usize,
}

impl ResourceRequest {
    pub fn cpu_heavy(cpu_threads: usize) -> Self {
        Self {
            cpu_threads: cpu_threads.max(1),
            io_slots: 0,
        }
    }

    pub fn io_heavy(cpu_threads: usize) -> Self {
        Self {
            cpu_threads: cpu_threads.max(1),
            io_slots: 1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResourceScheduler {
    cpu_threads: usize,
    io_slots: usize,
}

impl ResourceScheduler {
    pub fn automatic(profile: &HardwareProfile) -> Result<Self, String> {
        if profile.logical_cpu_threads == 0 {
            return Err("Hardware profile must report at least one CPU thread.".into());
        }
        Ok(Self {
            cpu_threads: profile.logical_cpu_threads,
            io_slots: 1,
        })
    }

    fn can_run(&self, request: ResourceRequest) -> bool {
        request.cpu_threads <= self.cpu_threads && request.io_slots <= self.io_slots
    }
}

#[derive(Debug)]
pub struct RuntimeCoordinator {
    scheduler: ResourceScheduler,
    jobs: HashSet<JobId>,
}

impl RuntimeCoordinator {
    pub fn new(scheduler: ResourceScheduler) -> Self {
        Self {
            scheduler,
            jobs: HashSet::new(),
        }
    }

    pub fn register_job(&mut self, job_id: JobId) -> Result<(), String> {
        if !self.jobs.insert(job_id) {
            return Err("Job is already registered with the runtime coordinator.".into());
        }
        Ok(())
    }

    pub(crate) fn acquire(
        &self,
        job_id: JobId,
        request: ResourceRequest,
    ) -> Result<ResourceLease, String> {
        if !self.jobs.contains(&job_id) {
            return Err("Job is not registered with the runtime coordinator.".into());
        }
        if !self.scheduler.can_run(request) {
            return Err("Requested resources are not currently available.".into());
        }
        Ok(ResourceLease)
    }
}

pub(crate) struct ResourceLease;
