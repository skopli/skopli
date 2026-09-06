use super::amp::AmpReader;
use super::augment::AugmentReader;
use super::cherrystudio::CherryStudioReader;
use super::claude::ClaudeReader;
use super::cline::ClineReader;
use super::codebuddy::CodebuddyReader;
use super::codebuff::CodebuffReader;
use super::codex::CodexReader;
use super::copilot::CopilotReader;
use super::deepseek::DeepseekReader;
use super::devin::DevinReader;
use super::droid::DroidReader;
use super::fx::FxReader;
use super::gemini::GeminiReader;
use super::goose::GooseReader;
use super::grok::GrokReader;
use super::hermes::HermesReader;
use super::jcode::JcodeReader;
use super::junie::JunieReader;
use super::kilo::KiloReader;
use super::kilocode::KilocodeReader;
use super::kimi::KimiReader;
use super::kiro::KiroReader;
use super::mux::MuxReader;
use super::openclaw::OpenclawReader;
use super::opencode::{CommandcodeReader, MimocodeReader, OpencodeReader};
use super::opencodereview::OpencodereviewReader;
use super::pi::{GajaeReader, KimchiReader, OmpReader, PiReader, PrimeReader};
use super::qwen::QwenReader;
use super::reader::Reader;
use super::reasonix::ReasonixReader;
use super::roo::RooReader;
use super::trae::TraeReader;
use super::zcode::ZcodeReader;
use super::zed::ZedReader;

/// The registered readers, one per wired harness. Adding a reader = adding one
/// entry here (plus its `impl Reader`); case discovery and dispatch pick it up
/// automatically, and any `golden/<harness>/` directory without a registered
/// reader is reported as pending rather than failing.
pub fn registered_readers() -> Vec<Box<dyn Reader>> {
    vec![
        Box::new(ClaudeReader),
        Box::new(CodexReader),
        Box::new(GeminiReader),
        Box::new(OpencodeReader),
        Box::new(MimocodeReader),
        Box::new(CommandcodeReader),
        Box::new(CopilotReader),
        Box::new(AmpReader),
        Box::new(DroidReader),
        Box::new(QwenReader),
        Box::new(PiReader),
        Box::new(OmpReader),
        Box::new(PrimeReader),
        Box::new(GajaeReader),
        Box::new(KimchiReader),
        Box::new(GrokReader),
        Box::new(AugmentReader),
        Box::new(CodebuffReader),
        Box::new(CodebuddyReader),
        Box::new(JcodeReader),
        Box::new(MuxReader),
        Box::new(ZcodeReader),
        Box::new(RooReader),
        Box::new(ClineReader),
        Box::new(KiloReader),
        Box::new(KilocodeReader),
        Box::new(OpenclawReader),
        Box::new(KimiReader),
        Box::new(JunieReader),
        Box::new(DevinReader),
        Box::new(HermesReader),
        Box::new(GooseReader),
        Box::new(ZedReader),
        Box::new(CherryStudioReader),
        Box::new(OpencodereviewReader),
        Box::new(TraeReader),
        Box::new(DeepseekReader),
        Box::new(ReasonixReader),
        Box::new(KiroReader),
        Box::new(FxReader),
    ]
}

/// The reader registered for `harness`, if any.
pub fn registered_reader(harness: &str) -> Option<Box<dyn Reader>> {
    registered_readers()
        .into_iter()
        .find(|r| r.harness_id() == harness)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn registry_has_40_unique_harness_ids() {
        let ids: Vec<&str> = registered_readers()
            .iter()
            .map(|r| r.harness_id())
            .collect();
        let unique: HashSet<&str> = ids.iter().copied().collect();
        assert_eq!(unique.len(), ids.len(), "harness ids must be unique");
        assert_eq!(unique.len(), 40);
        assert!(unique.contains("mimocode"));
        assert!(unique.contains("commandcode"));
        assert!(unique.contains("codebuddy"));
        assert!(unique.contains("jcode"));
        assert!(unique.contains("codebuff"));
        assert!(unique.contains("cherrystudio"));
        assert!(unique.contains("opencodereview"));
        assert!(unique.contains("trae"));
        assert!(unique.contains("deepseek"));
        assert!(unique.contains("reasonix"));
        assert!(unique.contains("kiro"));
        assert!(unique.contains("fx"));
    }
}
