// ── ail-stdlib::boundary ──────────────────────────────────────────────────
//
// Boundary and FFI helpers for the AIL `std.boundary` module.
//
// Used for:
// - FFI
// - external APIs
// - native extensions
// - LLM providers
// - OS/runtime integration

// ── TrustLevel ────────────────────────────────────────────────────────────

/// Trust classification for a boundary crossing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TrustLevel {
    /// Untrusted: adversarial or unknown.
    Untrusted,
    /// Third-party, partially trusted.
    External,
    /// Verified through schema or contract.
    Verified,
    /// Full trust — same process, same codebase.
    Trusted,
}

impl std::fmt::Display for TrustLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrustLevel::Trusted => write!(f, "trusted"),
            TrustLevel::Verified => write!(f, "verified"),
            TrustLevel::External => write!(f, "external"),
            TrustLevel::Untrusted => write!(f, "untrusted"),
        }
    }
}

// ── Assumption ────────────────────────────────────────────────────────────

/// A documented assumption at a boundary crossing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Assumption {
    pub id: String,
    pub description: String,
    pub trust_level: TrustLevel,
}

impl Assumption {
    pub fn new(
        id: impl Into<String>,
        description: impl Into<String>,
        trust_level: TrustLevel,
    ) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
            trust_level,
        }
    }
}

// ── ForeignType ───────────────────────────────────────────────────────────

/// Descriptor for a type originating outside the AIL type system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForeignType {
    pub name: String,
    pub origin: String,
    pub trust_level: TrustLevel,
}

impl ForeignType {
    pub fn new(
        name: impl Into<String>,
        origin: impl Into<String>,
        trust_level: TrustLevel,
    ) -> Self {
        Self {
            name: name.into(),
            origin: origin.into(),
            trust_level,
        }
    }
}

// ── ForeignFunction ───────────────────────────────────────────────────────

/// Descriptor for a function originating outside the AIL type system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForeignFunction {
    pub name: String,
    pub signature: String,
    pub trust_level: TrustLevel,
    pub assumptions: Vec<Assumption>,
}

impl ForeignFunction {
    pub fn new(
        name: impl Into<String>,
        signature: impl Into<String>,
        trust_level: TrustLevel,
    ) -> Self {
        Self {
            name: name.into(),
            signature: signature.into(),
            trust_level,
            assumptions: Vec::new(),
        }
    }

    pub fn with_assumption(mut self, assumption: Assumption) -> Self {
        self.assumptions.push(assumption);
        self
    }
}

// ── AdapterContract ───────────────────────────────────────────────────────

/// A contract governing the adapter between AIL code and a foreign boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdapterContract {
    pub id: String,
    pub ail_type: String,
    pub foreign_type: String,
    pub trust_level: TrustLevel,
    pub assumptions: Vec<Assumption>,
}

impl AdapterContract {
    pub fn new(
        id: impl Into<String>,
        ail_type: impl Into<String>,
        foreign_type: impl Into<String>,
        trust_level: TrustLevel,
    ) -> Self {
        Self {
            id: id.into(),
            ail_type: ail_type.into(),
            foreign_type: foreign_type.into(),
            trust_level,
            assumptions: Vec::new(),
        }
    }
}

// ── BoundaryDef ───────────────────────────────────────────────────────────

/// Full definition of a boundary, aggregating foreign types, functions, and contracts.
#[derive(Clone, Debug)]
pub struct BoundaryDef {
    pub id: String,
    pub trust_level: TrustLevel,
    pub foreign_types: Vec<ForeignType>,
    pub foreign_functions: Vec<ForeignFunction>,
    pub contracts: Vec<AdapterContract>,
}

impl BoundaryDef {
    pub fn new(id: impl Into<String>, trust_level: TrustLevel) -> Self {
        Self {
            id: id.into(),
            trust_level,
            foreign_types: Vec::new(),
            foreign_functions: Vec::new(),
            contracts: Vec::new(),
        }
    }
}

impl Default for BoundaryDef {
    fn default() -> Self {
        Self::new("default", TrustLevel::Untrusted)
    }
}
