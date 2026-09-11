use ai_novel_production_system_core::production::{
    ProductionModelGateway, ProductionModelResponse, ResolvedProviderProfile,
};
use serde_json::json;

pub struct EntryFixtureGateway;

impl ProductionModelGateway for EntryFixtureGateway {
    fn resolve_profile(&self) -> Result<ResolvedProviderProfile, String> {
        Ok(ResolvedProviderProfile {
            provider: "fixture-provider".into(),
            model: "fixture-model".into(),
            endpoint: "fixture://provider".into(),
            timeout_ms: 1_000,
        })
    }

    fn invoke(
        &self,
        capability: &str,
        input: &serde_json::Value,
    ) -> Result<ProductionModelResponse, String> {
        if capability != "GENERATE_STORY_DIRECTIONS" {
            return Err(format!("unexpected capability: {capability}"));
        }
        let source = input
            .pointer("/input/sourceText")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("测试故事想法");
        let pitches = (0..3)
            .map(|index| {
                json!({
                    "name": format!("测试方向 {}", index + 1),
                    "promise": format!("围绕 {source} 的故事承诺 {index}"),
                    "protagonistDesire": format!("主角目标 {source} {index}"),
                    "mechanism": format!("叙事机制 {source} {index}"),
                    "worldConflict": format!("世界冲突 {source} {index}"),
                    "longArc": format!("长线弧光 {source} {index}"),
                    "openingHook": format!("开篇钩子 {source} {index}"),
                    "volumeGoal": format!("卷目标 {source} {index}"),
                    "commercialEdge": format!("商业优势 {source} {index}"),
                    "originality": format!("原创性 {source} {index}"),
                    "difficulty": format!("难度 {source} {index}"),
                    "failureRisk": format!("失败风险 {source} {index}")
                })
            })
            .collect::<Vec<_>>();
        Ok(ProductionModelResponse::deterministic(json!({
            "pitches": pitches
        })))
    }
}
