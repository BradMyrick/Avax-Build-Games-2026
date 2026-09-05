//! Canonical data model for matchmaking: player tickets, rule sets, match
//! quality. Pure data — no async, no I/O, no crypto.

use serde::{Deserialize, Serialize};

pub type GameId = String;

/// Pure-data description of a player requesting a match. This is what the
/// matchmaking algorithm sees; it deliberately omits any server-side context
/// (notification channels, wallet signatures, session state) so the library
/// can be embedded anywhere.
///
/// In the AMP server, [`crate::QueueEntry`] wraps this with a notification
/// sender; library users use `PlayerTicket` directly.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlayerTicket {
    pub player_id: String,
    pub game_id: String,
    pub ruleset_id: String,
    pub mmr: f32,
    pub mmr_uncertainty: f32,
    pub region: String,
    pub preferred_role: String,
    pub language: String,
    pub max_ping_ms: u32,
    /// When this ticket entered the queue, in millis since UNIX_EPOCH. Used
    /// by rule evaluators (skill-decay widening, backfill trigger) and is
    /// required for the queue to compute queue duration correctly.
    pub enqueued_at_ms: u64,
}

impl AsRef<PlayerTicket> for PlayerTicket {
    fn as_ref(&self) -> &PlayerTicket {
        self
    }
}

/// A configured ruleset for a game mode. Holds the global skill window plus
/// the ordered list of evaluation rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleSet {
    pub ruleset_id: String,
    pub name: String,
    pub game_id: String,
    pub mode_id: String,
    pub rules: Vec<Rule>,
    pub timeout_ms: u64,
    pub backfill: Option<BackfillPolicy>,
    pub version: String,
    pub is_active: bool,
    pub max_skill_diff: f32,
}

impl RuleSet {
    /// Returns an `Arc<Self>` with rules sorted by (hard-constraint desc,
    /// priority asc) so evaluators can short-circuit on hard failures.
    pub fn new_sorted(mut self) -> std::sync::Arc<Self> {
        self.rules.sort_by(|a, b| {
            b.is_hard_constraint
                .cmp(&a.is_hard_constraint)
                .then_with(|| a.priority.cmp(&b.priority))
        });
        std::sync::Arc::new(self)
    }

    pub fn default_arc() -> std::sync::Arc<Self> {
        Self::default().new_sorted()
    }
}

impl Default for RuleSet {
    fn default() -> Self {
        Self {
            ruleset_id: String::new(),
            name: "default".to_string(),
            game_id: String::new(),
            mode_id: String::new(),
            rules: vec![Rule::default_skill()],
            timeout_ms: 30_000,
            backfill: None,
            version: "1.0".to_string(),
            is_active: true,
            max_skill_diff: 500.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub rule_id: String,
    pub name: String,
    pub rule_type: RuleType,
    pub params: RuleParams,
    pub weight: f32,
    pub is_hard_constraint: bool,
    pub priority: u8,
}

impl Rule {
    pub fn default_skill() -> Self {
        Self {
            rule_id: "default-skill".to_string(),
            name: "Skill Difference".to_string(),
            rule_type: RuleType::Skill,
            params: RuleParams::Skill(SkillParams {
                max_difference: 500.0,
                use_trueskill: false,
                team_variance: 0.0,
                time_decay: false,
                decay_rate: 0.0,
            }),
            weight: 0.70,
            is_hard_constraint: true,
            priority: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuleType {
    Latency,
    Skill,
    TeamBalance,
    Region,
    Language,
    Schedule,
    Inventory,
    Party,
    Avoidance,
    Preference,
    Custom,
    PingBased,
    SkillDecay,
    RecentMatches,
    ConnectionQuality,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuleParams {
    Latency(LatencyParams),
    Skill(SkillParams),
    TeamBalance(TeamBalanceParams),
    Region(RegionParams),
    Language(LanguageParams),
    Schedule(ScheduleParams),
    Inventory(InventoryParams),
    Party(PartyParams),
    Avoidance(AvoidanceParams),
    Preference(PreferenceParams),
    Custom(CustomParams),
    PingBased(PingBasedParams),
    SkillDecay(SkillDecayParams),
    RecentMatches(RecentMatchesParams),
    ConnectionQuality(ConnectionQualityParams),
}

impl Default for RuleParams {
    fn default() -> Self {
        RuleParams::Skill(SkillParams::default())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyParams {
    pub max_ping_ms: u32,
    pub measurement_method: String,
    pub allow_region_override: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillParams {
    pub max_difference: f32,
    /// Reserved: not implemented. The library currently ships 2-player
    /// Glicko-2; TrueSkill-style team Bayesian inference is a future feature.
    pub use_trueskill: bool,
    pub team_variance: f32,
    pub time_decay: bool,
    pub decay_rate: f32,
}

impl Default for SkillParams {
    fn default() -> Self {
        Self {
            max_difference: 500.0,
            use_trueskill: false,
            team_variance: 0.0,
            time_decay: false,
            decay_rate: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamBalanceParams {
    pub required_roles: Vec<String>,
    pub max_duplicates: u8,
    pub team_count: u8,
    pub flex_slots: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionParams {
    pub allowed_regions: Vec<String>,
    pub cross_region_after_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageParams {
    pub prefer_same: bool,
    pub weight: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleParams {
    pub time_windows: Vec<(u8, u8, u8)>,
    pub days_of_week: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryParams {
    pub required_items: Vec<String>,
    pub banned_items: Vec<String>,
    pub min_collection_score: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartyParams {
    pub allow_mixed: bool,
    pub max_size: u8,
    pub skill_method: PartySkillMethod,
    pub solo_queue_bonus: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum PartySkillMethod {
    Highest,
    Average,
    Weighted,
    AdjustedAverage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvoidanceParams {
    pub max_recent_opponents: u8,
    pub avoid_blocked: bool,
    pub cooldown_minutes: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreferenceParams {
    pub prefer_new_opponents: bool,
    pub prefer_similar_playtime: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomParams {
    pub evaluator_id: String,
    pub config: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingBasedParams {
    pub max_ping_ms: u32,
    pub region_override: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDecayParams {
    pub inactive_days: u32,
    pub decay_per_day: f32,
    pub min_mmr: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentMatchesParams {
    pub max_with_same_opponent: u8,
    pub window_minutes: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionQualityParams {
    pub max_packet_loss: f32,
    pub max_jitter_ms: u32,
    pub min_bandwidth_kbps: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackfillPolicy {
    pub enabled: bool,
    pub max_time_ms: u64,
    pub skill_tolerance_multiplier: f32,
    pub partial_teams: bool,
    pub role_flexibility: bool,
    pub connection_tolerance: bool,
}

impl Default for BackfillPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            max_time_ms: 15_000,
            skill_tolerance_multiplier: 1.5,
            partial_teams: true,
            role_flexibility: true,
            connection_tolerance: true,
        }
    }
}

/// Quality breakdown of a produced pairing. Returned by [`crate::evaluate_rules`]
/// and surfaced on the AMP `MatchFoundPayload`. Higher = better.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MatchQualityDetail {
    pub total_score: f32,
    pub skill_balance: f32,
    pub role_balance: f32,
    pub region_score: f32,
    pub latency_score: f32,
    pub language_score: f32,
}
