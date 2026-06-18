#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoltageSet {
    V208,
    V415,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadLevel {
    Full,
    Half,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskKind {
    Transformer {
        voltage: VoltageSet,
    },
    System {
        voltage: VoltageSet,
        load: LoadLevel,
    },
    Breaker {
        voltage: VoltageSet,
        breaker: u8,
        load: LoadLevel,
    },
    SystemBurnIn,
    BreakerBurnIn {
        breaker: u8,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationTask {
    pub id: String,
    pub label: String,
    pub step_display: String,
    pub detection_steps: Vec<u16>,
    pub kind: TaskKind,
}

impl VoltageSet {
    pub fn prefix(self) -> &'static str {
        match self {
            VoltageSet::V208 => "208v",
            VoltageSet::V415 => "415v",
        }
    }

    pub fn display(self) -> &'static str {
        match self {
            VoltageSet::V208 => "208V",
            VoltageSet::V415 => "415V",
        }
    }
}

impl LoadLevel {
    pub fn display(self) -> &'static str {
        match self {
            LoadLevel::Full => "100% Load",
            LoadLevel::Half => "50% Load",
            LoadLevel::Low => "20% Load",
        }
    }

    pub fn script_arg(self) -> &'static str {
        match self {
            LoadLevel::Full => "100%",
            LoadLevel::Half => "50%",
            LoadLevel::Low => "20%",
        }
    }

    pub fn system_test_name(self) -> &'static str {
        match self {
            LoadLevel::Full => "100% Load Test",
            LoadLevel::Half => "50% Load Test",
            LoadLevel::Low => "20% Load Test",
        }
    }

    pub fn index(self) -> u16 {
        match self {
            LoadLevel::Full => 0,
            LoadLevel::Half => 1,
            LoadLevel::Low => 2,
        }
    }
}

pub fn automation_tasks() -> Vec<AutomationTask> {
    let mut tasks = Vec::new();

    tasks.push(AutomationTask {
        id: "208v-transformer".to_string(),
        label: "208V Transformer Check".to_string(),
        step_display: "14".to_string(),
        detection_steps: vec![14],
        kind: TaskKind::Transformer {
            voltage: VoltageSet::V208,
        },
    });

    add_system_tasks(&mut tasks, VoltageSet::V208, 15);
    add_breaker_tasks(&mut tasks, VoltageSet::V208, 18);

    tasks.push(AutomationTask {
        id: "415v-transformer".to_string(),
        label: "415V Transformer Check".to_string(),
        step_display: "43".to_string(),
        detection_steps: vec![43],
        kind: TaskKind::Transformer {
            voltage: VoltageSet::V415,
        },
    });

    add_system_tasks(&mut tasks, VoltageSet::V415, 44);
    add_breaker_tasks(&mut tasks, VoltageSet::V415, 47);

    tasks.push(AutomationTask {
        id: "system-burn-in".to_string(),
        label: "System Burn-In".to_string(),
        step_display: "71/72".to_string(),
        detection_steps: vec![71, 72],
        kind: TaskKind::SystemBurnIn,
    });

    for breaker in 1..=8 {
        let step = 72 + u16::from(breaker);
        tasks.push(AutomationTask {
            id: format!("breaker-burn-in-{breaker}"),
            label: format!("Breaker {breaker}"),
            step_display: step.to_string(),
            detection_steps: vec![step],
            kind: TaskKind::BreakerBurnIn { breaker },
        });
    }

    tasks
}

pub fn find_task(task_id: &str) -> Option<AutomationTask> {
    automation_tasks()
        .into_iter()
        .find(|task| task.id == task_id)
}

pub fn step_for_system(voltage: VoltageSet, load: LoadLevel) -> u16 {
    let start = match voltage {
        VoltageSet::V208 => 15,
        VoltageSet::V415 => 44,
    };

    start + load.index()
}

pub fn step_for_breaker(voltage: VoltageSet, breaker: u8, load: LoadLevel) -> u16 {
    let start = match voltage {
        VoltageSet::V208 => 18,
        VoltageSet::V415 => 47,
    };

    start + (u16::from(breaker) - 1) * 3 + load.index()
}

fn add_system_tasks(tasks: &mut Vec<AutomationTask>, voltage: VoltageSet, first_step: u16) {
    for load in [LoadLevel::Full, LoadLevel::Half, LoadLevel::Low] {
        let step = first_step + load.index();
        tasks.push(AutomationTask {
            id: format!("{}-system-{}", voltage.prefix(), load.display()),
            label: load.display().to_string(),
            step_display: step.to_string(),
            detection_steps: vec![step],
            kind: TaskKind::System { voltage, load },
        });
    }
}

fn add_breaker_tasks(tasks: &mut Vec<AutomationTask>, voltage: VoltageSet, first_step: u16) {
    for breaker in 1..=8 {
        for load in [LoadLevel::Full, LoadLevel::Half, LoadLevel::Low] {
            let step = first_step + (u16::from(breaker) - 1) * 3 + load.index();
            tasks.push(AutomationTask {
                id: format!(
                    "{}-breaker-{}-{}",
                    voltage.prefix(),
                    breaker,
                    load.display()
                ),
                label: load.display().to_string(),
                step_display: step.to_string(),
                detection_steps: vec![step],
                kind: TaskKind::Breaker {
                    voltage,
                    breaker,
                    load,
                },
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_map_matches_legacy_count_and_key_steps() {
        let tasks = automation_tasks();

        assert_eq!(tasks.len(), 65);
        assert_eq!(tasks[0].id, "208v-transformer");
        assert_eq!(tasks[0].detection_steps, vec![14]);
        assert_eq!(tasks[64].id, "breaker-burn-in-8");
        assert_eq!(tasks[64].detection_steps, vec![80]);
        assert_eq!(step_for_breaker(VoltageSet::V415, 8, LoadLevel::Low), 70);
    }
}
