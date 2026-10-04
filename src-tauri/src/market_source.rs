//! B-R2-04A's bounded real-source adapter.
//!
//! The adapter reads one public, official Fanqie writer-zone page per explicit
//! user action. It keeps only normalized category counts and links to official
//! policy notices; publication titles and article bodies are never passed to a
//! model or promoted to story authority.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub const FANQIE_WRITER_ZONE_URL: &str = "https://fanqienovel.com/writer/zone/";
const FANQIE_SOURCE_ID: &str = "fanqie-writer-zone";
const MAX_SOURCE_BYTES: usize = 1_048_576;
const MAX_PUBLICATION_SAMPLE: usize = 100;
const MAX_POLICY_REFERENCES: usize = 12;
const MARKET_SNAPSHOT_MAX_AGE_MS: i64 = 7 * 24 * 60 * 60 * 1000;
const PLATFORM_POLICY_MAX_AGE_MS: i64 = 30 * 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarketFreshness {
    pub market_stale: bool,
    pub policy_stale: bool,
}

pub fn freshness(
    source_status: &str,
    policy_status: &str,
    market_observed_at_epoch_ms: i64,
    policy_observed_at_epoch_ms: i64,
    now_epoch_ms: i64,
) -> MarketFreshness {
    MarketFreshness {
        market_stale: source_status == "STALE"
            || (source_status == "READY"
                && now_epoch_ms.saturating_sub(market_observed_at_epoch_ms)
                    > MARKET_SNAPSHOT_MAX_AGE_MS),
        policy_stale: policy_status == "STALE"
            || (policy_status == "INDEXED"
                && now_epoch_ms.saturating_sub(policy_observed_at_epoch_ms)
                    > PLATFORM_POLICY_MAX_AGE_MS),
    }
}

pub fn ensure_opportunity_selectable(
    source_status: &str,
    policy_status: &str,
    market_observed_at_epoch_ms: i64,
    policy_observed_at_epoch_ms: i64,
    now_epoch_ms: i64,
) -> Result<(), &'static str> {
    let freshness = freshness(
        source_status,
        policy_status,
        market_observed_at_epoch_ms,
        policy_observed_at_epoch_ms,
        now_epoch_ms,
    );
    if freshness.market_stale {
        return Err("MARKET_OPPORTUNITY_STALE");
    }
    if source_status != "READY" {
        return Err("MARKET_SOURCE_UNAVAILABLE");
    }
    if freshness.policy_stale {
        return Err("MARKET_POLICY_STALE");
    }
    Ok(())
}

pub trait MarketSourceAdapter: Send + Sync {
    fn fetch(&self) -> MarketSourceSnapshot;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MarketSourceSnapshot {
    pub source_status: String,
    pub source_id: String,
    pub source_url: String,
    pub source_digest: Option<String>,
    pub observed_at_epoch_ms: i64,
    pub sample_count: usize,
    pub categories: Vec<MarketCategoryObservation>,
    pub policy_status: String,
    pub policy_source_url: String,
    pub policy_digest: Option<String>,
    pub policy_observed_at_epoch_ms: i64,
    pub policy_references: Vec<MarketPolicyReference>,
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MarketCategoryObservation {
    pub topic_id: String,
    pub category: String,
    pub publication_count: usize,
    pub top_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MarketPolicyReference {
    pub title: String,
    pub url: String,
    pub published_at: Option<String>,
    pub external_text_classification: String,
}

pub struct UnavailableMarketSourceAdapter;

impl MarketSourceAdapter for UnavailableMarketSourceAdapter {
    fn fetch(&self) -> MarketSourceSnapshot {
        MarketSourceSnapshot::unavailable("MARKET_SOURCE_NOT_CONFIGURED")
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct FanqieWriterZoneAdapter;

impl MarketSourceAdapter for FanqieWriterZoneAdapter {
    fn fetch(&self) -> MarketSourceSnapshot {
        let observed_at = current_epoch_ms();
        match fetch_writer_zone_page() {
            Ok(body) => parse_fanqie_writer_zone(&body, observed_at).unwrap_or_else(|_| {
                MarketSourceSnapshot::unavailable("MARKET_SOURCE_RESPONSE_INVALID")
            }),
            Err(code) => MarketSourceSnapshot::unavailable(code),
        }
    }
}

impl MarketSourceSnapshot {
    pub fn unavailable(error_code: &str) -> Self {
        let now = current_epoch_ms();
        Self {
            source_status: "SOURCE_UNAVAILABLE".into(),
            source_id: FANQIE_SOURCE_ID.into(),
            source_url: FANQIE_WRITER_ZONE_URL.into(),
            source_digest: None,
            observed_at_epoch_ms: now,
            sample_count: 0,
            categories: Vec::new(),
            policy_status: "SOURCE_UNAVAILABLE".into(),
            policy_source_url: FANQIE_WRITER_ZONE_URL.into(),
            policy_digest: None,
            policy_observed_at_epoch_ms: now,
            policy_references: Vec::new(),
            error_code: Some(error_code.to_string()),
        }
    }
}

pub fn current_epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

fn fetch_writer_zone_page() -> Result<String, &'static str> {
    const MARKER: &str = "\n__MOHENG_MARKET_HTTP_META__";
    let output = Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--connect-timeout",
            "4",
            "--max-time",
            "10",
            "--max-filesize",
            "1048576",
            "--proto",
            "=https",
            "--max-redirs",
            "0",
            "--user-agent",
            "Moheng/2.1 (human-initiated market discovery)",
            "--header",
            "Accept: text/html",
            "--write-out",
            "\n__MOHENG_MARKET_HTTP_META__%{http_code}|%{url_effective}|%{content_type}",
            FANQIE_WRITER_ZONE_URL,
        ])
        .output()
        .map_err(|_| "MARKET_SOURCE_CLIENT_UNAVAILABLE")?;
    if !output.status.success() || output.stdout.len() > MAX_SOURCE_BYTES + 256 {
        return Err("MARKET_SOURCE_HTTP_UNAVAILABLE");
    }
    let response =
        String::from_utf8(output.stdout).map_err(|_| "MARKET_SOURCE_RESPONSE_ENCODING_INVALID")?;
    let (body, metadata) = response
        .rsplit_once(MARKER)
        .ok_or("MARKET_SOURCE_RESPONSE_METADATA_MISSING")?;
    let mut fields = metadata.splitn(3, '|');
    let status = fields.next().unwrap_or_default();
    let final_url = fields.next().unwrap_or_default();
    let content_type = fields.next().unwrap_or_default();
    if status != "200"
        || final_url != FANQIE_WRITER_ZONE_URL
        || !content_type.starts_with("text/html")
        || body.len() > MAX_SOURCE_BYTES
    {
        return Err("MARKET_SOURCE_HTTP_UNAVAILABLE");
    }
    Ok(body.to_string())
}

/// Parses only the page's bounded JSON-backed display data. It never parses or
/// executes scripts, follows page-provided URLs, or returns publication titles.
pub fn parse_fanqie_writer_zone(
    html: &str,
    observed_at_epoch_ms: i64,
) -> Result<MarketSourceSnapshot, String> {
    if html.len() > MAX_SOURCE_BYTES {
        return Err("MARKET_SOURCE_RESPONSE_TOO_LARGE".into());
    }
    let marker = "window._ROUTER_DATA = ";
    let start = html
        .find(marker)
        .ok_or_else(|| "MARKET_SOURCE_ROUTER_DATA_MISSING".to_string())?
        + marker.len();
    let tail = &html[start..];
    let end = tail
        .find("</script>")
        .ok_or_else(|| "MARKET_SOURCE_ROUTER_DATA_UNTERMINATED".to_string())?;
    let json_text = tail[..end]
        .trim()
        .strip_suffix(';')
        .unwrap_or(tail[..end].trim());
    let value: Value = serde_json::from_str(json_text)
        .map_err(|_| "MARKET_SOURCE_ROUTER_DATA_INVALID".to_string())?;
    let home = value
        .pointer("/loaderData/page/home")
        .and_then(Value::as_object)
        .ok_or_else(|| "MARKET_SOURCE_HOME_DATA_MISSING".to_string())?;

    let publication_rows = home
        .get("publicationList")
        .and_then(Value::as_array)
        .ok_or_else(|| "MARKET_SOURCE_PUBLICATION_SAMPLE_MISSING".to_string())?;
    let sample_count = publication_rows.len().min(MAX_PUBLICATION_SAMPLE);
    let mut category_counts: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for item in publication_rows.iter().take(MAX_PUBLICATION_SAMPLE) {
        let Some(category) = item
            .get("book_type")
            .and_then(Value::as_str)
            .and_then(clean_external_label)
        else {
            continue;
        };
        let entry = category_counts.entry(category).or_insert((0, 0));
        entry.0 += 1;
        if item.get("is_top").and_then(Value::as_str) == Some("是") {
            entry.1 += 1;
        }
    }
    let categories = category_counts
        .into_iter()
        .map(|(category, (publication_count, top_count))| {
            let digest = sha256_hex(category.as_bytes());
            MarketCategoryObservation {
                topic_id: format!("fanqie:publication:{}", &digest[..16]),
                category,
                publication_count,
                top_count,
            }
        })
        .collect::<Vec<_>>();

    let policy_references = policy_references(home);
    let source_digest = sha256_hex(html.as_bytes());
    let source_status = if categories.is_empty() {
        "SOURCE_UNAVAILABLE"
    } else {
        "READY"
    };
    let policy_status = if policy_references.is_empty() {
        "SOURCE_UNAVAILABLE"
    } else {
        "INDEXED"
    };
    Ok(MarketSourceSnapshot {
        source_status: source_status.into(),
        source_id: FANQIE_SOURCE_ID.into(),
        source_url: FANQIE_WRITER_ZONE_URL.into(),
        source_digest: Some(source_digest.clone()),
        observed_at_epoch_ms,
        sample_count,
        categories,
        policy_status: policy_status.into(),
        policy_source_url: FANQIE_WRITER_ZONE_URL.into(),
        policy_digest: (!policy_references.is_empty()).then_some(source_digest),
        policy_observed_at_epoch_ms: observed_at_epoch_ms,
        policy_references,
        error_code: (source_status == "SOURCE_UNAVAILABLE")
            .then(|| "MARKET_SOURCE_PUBLICATION_SAMPLE_EMPTY".into()),
    })
}

fn clean_external_label(raw: &str) -> Option<String> {
    let cleaned = raw
        .chars()
        .filter(|character| !character.is_control())
        .take(60)
        .collect::<String>()
        .trim()
        .to_string();
    (!cleaned.is_empty()).then_some(cleaned)
}

fn policy_references(home: &serde_json::Map<String, Value>) -> Vec<MarketPolicyReference> {
    let mut references = Vec::new();
    for list_name in ["noticeList", "beginnerList"] {
        let Some(items) = home.get(list_name).and_then(Value::as_array) else {
            continue;
        };
        for item in items {
            let Some(title) = item
                .get("title")
                .and_then(Value::as_str)
                .and_then(clean_external_label)
            else {
                continue;
            };
            if !is_policy_reference_title(&title) {
                continue;
            }
            let Some(url) = item
                .get("url")
                .or_else(|| item.get("link"))
                .and_then(Value::as_str)
                .filter(|url| safe_policy_url(url))
            else {
                continue;
            };
            let published_at = item
                .get("time")
                .and_then(Value::as_str)
                .or_else(|| item.get("publishedAt").and_then(Value::as_str))
                .map(|value| value.chars().take(32).collect());
            if references
                .iter()
                .any(|reference: &MarketPolicyReference| reference.url == url)
            {
                continue;
            }
            references.push(MarketPolicyReference {
                title,
                url: url.to_string(),
                published_at,
                external_text_classification: "DATA_ONLY".into(),
            });
            if references.len() >= MAX_POLICY_REFERENCES {
                break;
            }
        }
        if references.len() >= MAX_POLICY_REFERENCES {
            break;
        }
    }
    references
}

fn is_policy_reference_title(title: &str) -> bool {
    ["平台规则", "签约", "治理公告", "创作须知", "违规处罚"]
        .iter()
        .any(|marker| title.contains(marker))
}

fn safe_policy_url(url: &str) -> bool {
    url.len() <= 512
        && url.starts_with("https://fanqienovel.com/writer/zone/article/")
        && !url.contains('@')
        && !url
            .chars()
            .any(|character| matches!(character, '\\' | '\n' | '\r' | '\0'))
        && !url.contains('?')
        && !url.contains('#')
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn discovery_projection(snapshot: &MarketSourceSnapshot) -> Value {
    let opportunities = if snapshot.source_status == "READY" {
        snapshot
            .categories
            .iter()
            .map(|category| opportunity_value(snapshot, category))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    json!({
        "status":snapshot.source_status,
        "source":{
            "sourceId":snapshot.source_id,
            "sourceUrl":snapshot.source_url,
            "digest":snapshot.source_digest,
            "observedAtEpochMs":snapshot.observed_at_epoch_ms,
            "sampleCount":snapshot.sample_count,
            "metricsBasis":"OFFICIAL_PUBLICATION_SAMPLE"
        },
        "policyStatus":snapshot.policy_status,
        "policySourceUrl":snapshot.policy_source_url,
        "policyDigest":snapshot.policy_digest,
        "policyObservedAtEpochMs":snapshot.policy_observed_at_epoch_ms,
        "policyReferences":snapshot.policy_references,
        "errorCode":snapshot.error_code,
        "opportunities":opportunities,
        "limitations":[
            "此来源是平台公开出版作品展示样本，不是完整书库或榜单。",
            "来源未提供阅读量、销量、签约率或收益数据；相关数值保持未提供。",
            "外部标题与规则公告仅作为 DATA_ONLY 资料，不会被解释为系统指令或作品事实。"
        ]
    })
}

pub fn opportunity_value(
    snapshot: &MarketSourceSnapshot,
    category: &MarketCategoryObservation,
) -> Value {
    let evidence = format!(
        "番茄作家专区公开出版展示：样本 {} 项，其中分类“{}”出现 {} 项、标记精选 {} 项。此为展示样本，不代表销量、阅读量或榜单。",
        snapshot.sample_count, category.category, category.publication_count, category.top_count
    );
    json!({
        "id":category.topic_id,
        "kind":"官方展示",
        "title":format!("官方出版展示观察 · {}", category.category),
        "direction":category.category,
        "platform":"番茄小说",
        "audience":"未提供",
        "demand":null,"competition":null,"homogeneityRisk":null,"saturation":null,
        "growth":null,"monetization":null,"newcomerDifficulty":null,"longFormCapacity":null,
        "originalitySpace":null,"aiFeasibility":null,"complianceRisk":null,
        "dataConfidence":null,"recommendationConfidence":null,
        "evidence":[evidence],
        "recommendWhy":"该类别出现在番茄官方公开出版展示样本中，可作为进一步研究线索；样本不代表市场需求或商业表现。",
        "notRecommendWhy":"来源不含阅读量、销量、签约率、稿酬或完整排名，不能据此评估变现或成功概率。",
        "source":"番茄作家专区·公开出版作品展示",
        "collectedAtEpochMs":snapshot.observed_at_epoch_ms,
        "sourceDigest":snapshot.source_digest,
        "policyVersion":snapshot.policy_digest,
        "metricsBasis":"OFFICIAL_PUBLICATION_SAMPLE",
        "sampleCount":snapshot.sample_count,
        "categorySampleCount":category.publication_count,
        "topMarkedCount":category.top_count,
        "stale":false,
        "externalTextClassification":"DATA_ONLY"
    })
}
