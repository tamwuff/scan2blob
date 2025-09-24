pub mod web;

#[derive(serde::Deserialize)]
pub struct ConfigGate {
    #[serde(default = "default_default_open")]
    pub default_open: bool,
    #[serde(default = "default_timed_assertion_lifetime")]
    pub timed_assertion_lifetime: u32,
    #[serde(default = "default_name_hint_lifetime")]
    pub name_hint_lifetime: u32,
    pub web_ui: Option<web::ConfigGateWeb>,
}

pub type ConfigGates = std::collections::HashMap<String, ConfigGate>;

fn default_default_open() -> bool {
    false
}

fn default_timed_assertion_lifetime() -> u32 {
    // 1 hour
    3600
}

fn default_name_hint_lifetime() -> u32 {
    // 10 minutes
    600
}

// On a conceptual level, the way we want the system to behave is, it's as if
// there can be two different kinds of gate-open assertions, one kind that
// has an explicit expiration time, and a different kind that is associated
// with a guard object and which lives as long as the guard object does.
//
// Again on a conceptual level, we would like the system to behave as if
// name-hints were scoped within gate-open assertions. If a gate-open assertion
// expires or its guard object is dropped, we would like its name-hint (if it
// had one) to expire "early", even if the name-hint wouldn't otherwise have
// expired yet.
//
// The way this is actually implemented is as follows:
//
// 1. Gate-open assertions with guard objects and no explicit expiration time,
//    are represented by a single HashSet<u64>. If the length of the HashSet is
//    greater than zero, there is at least one such active gate-open assertion.
//
// 2. Gate-open assertions with an explicit expiration time, are represented by
//    a single Option<Instant>. If someone else comes along and asserts that
//    the gate should be open for a little while longer after the previous
//    expiration time, we just overwrite the new expiration time on top of the
//    previous one.
//
// 3. The currently active name-hint, if there is one, is stored along with
//    its expiration time and an Option<u64> which may be used to indicate that
//    this name-hint depends on a particular gate-open assertion that has a
//    guard object.
//
// There's only one other enigma to explain, and that is the "sentinel". If
// "sentinel" is true, we consider the gate to be open even if no other
// assertions say it is. We initialize "sentinel" with whatever value the
// configuration's "default_open" field has. We do the same thing whenever
// anyone creates a gate-open assertion of any sort, whether expiring or
// guarded. We just overwrite it with whatever the value of "default_open" is.
// If we ever assert explicitly that the gate is closed, we set "sentinel" to
// false. The next person to make a gate-open assertion after that, will cause
// it to go back to whatever the default was.

pub struct GateAssertionGuard {
    gate: std::sync::Arc<Gate>,
    id: u64,
}

impl Drop for GateAssertionGuard {
    fn drop(&mut self) {
        let mut inner = self.gate.inner.write().unwrap();
        let _ = inner.guarded_assertions.remove(&self.id);
        if let Some(ref name_hint) = inner.name_hint {
            if let Some(depends_on_guarded) = name_hint.depends_on_guarded {
                if depends_on_guarded == self.id {
                    inner.name_hint = None;
                }
            }
        }
    }
}

struct GateNameHint {
    expires_at: std::time::Instant,
    depends_on_guarded: Option<u64>,
    name_hint: String,
}

pub struct GateInner {
    sentinel: bool,
    next_guarded_assertion_id: u64,
    guarded_assertions: std::collections::HashSet<u64>,
    expiring_assertion: Option<std::time::Instant>,
    name_hint: Option<GateNameHint>,
}

pub struct Gate {
    ctx: std::sync::Arc<crate::ctx::Ctx>,
    pub name: String,
    default_open: bool,
    timed_assertion_lifetime: std::time::Duration,
    name_hint_lifetime: std::time::Duration,
    inner: std::sync::RwLock<GateInner>,
}

impl Gate {
    pub fn new(
        ctx: &std::sync::Arc<crate::ctx::Ctx>,
        name: &String,
        cfg: &ConfigGate,
    ) -> Result<Self, scan2blob::error::WuffError> {
        let inner: GateInner = GateInner {
            sentinel: cfg.default_open,
            next_guarded_assertion_id: 0,
            guarded_assertions: std::collections::HashSet::new(),
            expiring_assertion: None,
            name_hint: None,
        };
        Ok(Self {
            ctx: std::sync::Arc::clone(ctx),
            name: name.clone(),
            default_open: cfg.default_open,
            timed_assertion_lifetime: std::time::Duration::from_secs(
                cfg.timed_assertion_lifetime as u64,
            ),
            name_hint_lifetime: std::time::Duration::from_secs(
                cfg.name_hint_lifetime as u64,
            ),
            inner: std::sync::RwLock::new(inner),
        })
    }

    pub fn assert_gate_closed(&self) {
        let mut inner = self.inner.write().unwrap();
        inner.sentinel = false;
        inner.guarded_assertions.clear();
        inner.expiring_assertion = None;
        inner.name_hint = None;
    }

    pub fn assert_gate_open_timed(&self) {
        let mut inner = self.inner.write().unwrap();
        inner.sentinel = self.default_open;
        inner.expiring_assertion =
            Some(std::time::Instant::now() + self.timed_assertion_lifetime);
    }

    pub fn assert_gate_open_timed_with_name_hint(
        &self,
        name_hint: Option<String>,
    ) {
        let mut inner = self.inner.write().unwrap();
        inner.sentinel = self.default_open;
        let now: std::time::Instant = std::time::Instant::now();
        let expiration: std::time::Instant =
            now + self.timed_assertion_lifetime;
        inner.expiring_assertion = Some(expiration);
        inner.name_hint = if let Some(name_hint) = name_hint {
            let name_hint_expiration: std::time::Instant =
                std::cmp::min(now + self.name_hint_lifetime, expiration);
            Some(GateNameHint {
                expires_at: name_hint_expiration,
                depends_on_guarded: None,
                name_hint,
            })
        } else {
            None
        };
    }

    pub fn assert_gate_open_guarded(
        self: &std::sync::Arc<Self>,
    ) -> GateAssertionGuard {
        let mut inner = self.inner.write().unwrap();
        inner.sentinel = self.default_open;
        let id: u64 = inner.next_guarded_assertion_id;
        inner.next_guarded_assertion_id += 1;
        assert!(inner.guarded_assertions.insert(id));

        GateAssertionGuard {
            gate: std::sync::Arc::clone(self),
            id,
        }
    }

    pub fn assert_gate_open_guarded_with_name_hint(
        self: &std::sync::Arc<Self>,
        name_hint: Option<String>,
    ) -> GateAssertionGuard {
        let mut inner = self.inner.write().unwrap();
        inner.sentinel = self.default_open;
        let id: u64 = inner.next_guarded_assertion_id;
        inner.next_guarded_assertion_id += 1;
        assert!(inner.guarded_assertions.insert(id));
        inner.name_hint = if let Some(name_hint) = name_hint {
            let name_hint_expiration: std::time::Instant =
                std::time::Instant::now() + self.name_hint_lifetime;
            Some(GateNameHint {
                expires_at: name_hint_expiration,
                depends_on_guarded: Some(id),
                name_hint,
            })
        } else {
            None
        };

        GateAssertionGuard {
            gate: std::sync::Arc::clone(self),
            id,
        }
    }

    pub fn get_current_state(&self) -> Option<Option<String>> {
        let (state, _next_change_time) = self.get_current_state_extended();
        state
    }

    pub fn get_current_state_extended(
        &self,
    ) -> (Option<Option<String>>, Option<std::time::Duration>) {
        let mut time_until_gate_closes: Option<std::time::Duration> = None;
        let inner = self.inner.read().unwrap();
        let now: std::time::Instant = std::time::Instant::now();
        let mut gate_open: bool =
            inner.sentinel || !inner.guarded_assertions.is_empty();
        if !gate_open {
            if let Some(expiring_assertion) = inner.expiring_assertion {
                if let time_left @ Some(_) =
                    expiring_assertion.checked_duration_since(now)
                {
                    gate_open = true;
                    time_until_gate_closes = time_left;
                }
            }
        }
        if !gate_open {
            return (None, None);
        }
        if let Some(ref name_hint) = inner.name_hint {
            if let Some(mut time_until_something_happens) =
                name_hint.expires_at.checked_duration_since(now)
            {
                if let Some(time_until_gate_closes) = time_until_gate_closes {
                    time_until_something_happens = std::cmp::min(
                        time_until_something_happens,
                        time_until_gate_closes,
                    );
                }
                return (
                    Some(Some(name_hint.name_hint.clone())),
                    Some(time_until_something_happens),
                );
            }
        }
        (Some(None), time_until_gate_closes)
    }

    pub fn try_write_file(
        &self,
        orig_filename: &str,
        destination: &std::sync::Arc<crate::destination::Destination>,
    ) -> Option<scan2blob::chunker::Writer> {
        let Some(name_hint) = self.get_current_state() else {
            return None;
        };
        let Some(mime_type) = self.ctx.mime_types.get(orig_filename) else {
            return None;
        };
        Some(destination.write_file(
            name_hint,
            mime_type.suffix,
            mime_type.content_type,
        ))
    }
}

pub struct Gates {
    gates: std::collections::HashMap<String, std::sync::Arc<Gate>>,
}

impl Gates {
    pub fn new(
        ctx: &std::sync::Arc<crate::ctx::Ctx>,
    ) -> Result<Self, scan2blob::error::WuffError> {
        let mut gates: std::collections::HashMap<
            String,
            std::sync::Arc<Gate>,
        > = std::collections::HashMap::new();
        for (gate_name, gate_cfg) in &ctx.config.gates {
            let gate: Gate = Gate::new(ctx, gate_name, gate_cfg)?;
            assert!(
                gates
                    .insert(gate_name.clone(), std::sync::Arc::new(gate))
                    .is_none()
            );
        }
        Ok(Self { gates })
    }
    pub fn get(&self, name: &str) -> Option<std::sync::Arc<Gate>> {
        self.gates.get(name).cloned()
    }
}
