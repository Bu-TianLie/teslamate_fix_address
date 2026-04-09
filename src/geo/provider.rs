use anyhow::Result;
use async_trait::async_trait;

/// Normalized geocode result.
#[derive(Debug, Clone)]
pub struct GeocodeResult {
 
    pub province: Option<String>,// 省份

    pub display_name: String, // 地址名称
    pub name: Option<String>, // 地址名称
    pub latitude: Option<f64>, // 纬度
    pub longitude: Option<f64>, // 经度
    pub house_number: Option<String>, // 门号
    pub road: Option<String>, // 道路
    pub city: Option<String>, // 城市
    pub postcode: Option<String>, // 邮编
    pub country: Option<String>, // 国家
    pub neighbourhood: Option<String>, // 小区
    pub county: Option<String>, // 县区
    pub state_district: Option<String>, // 市区
    pub raw: serde_json::Value, // 原始数据
}

/// Coordinate system used by the provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordSystem {
    Wgs84,
    Gcj02,
    Bd09,
}

/// Plugin trait for geocoding providers.
#[async_trait]
pub trait GeoProvider: Send + Sync {
    /// Provider name (e.g. "tencent").
    fn name(&self) -> &str;

    /// Coordinate system the API expects.
    fn coord_system(&self) -> CoordSystem;

    /// Reverse geocode a WGS84 coordinate.
    /// Implementations handle coordinate conversion internally.
    async fn reverse_geocode(&self, lat: f64, lng: f64) -> Result<GeocodeResult>;
}
