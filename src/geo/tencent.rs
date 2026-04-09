use std::time::Duration;

use anyhow::{bail, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::debug;
use super::provider::{CoordSystem, GeocodeResult, GeoProvider};

pub struct TencentProvider {
    key: String,
    client: Client,
}

impl TencentProvider {
    pub fn new(key: String) -> Self {
        Self {
            key,
            client: Client::builder()
                .timeout(Duration::from_secs(10))
                .connect_timeout(Duration::from_secs(5))
                .build()
                .expect("failed to build reqwest client"),
        }
    }
}

#[async_trait]
impl GeoProvider for TencentProvider {
    fn name(&self) -> &str {
        "tencent"
    }

    fn coord_system(&self) -> CoordSystem {
        CoordSystem::Gcj02
    }

    async fn reverse_geocode(&self, lat: f64, lng: f64) -> Result<GeocodeResult> {
        // let (glat, glng) = wgs84_to_gcj02(lat, lng);
        let location = format!("{:.6},{:.6}", lat, lng);

        let resp: TencentResponse = self
            .client
            .get("https://apis.map.qq.com/ws/geocoder/v1/")
            .query(&[("location", location.as_str()), ("key", self.key.as_str()), ("get_poi", "0"),("coord_type", "1")])
            .send()
            .await?
            .json()
            .await?;

        if resp.status != 0 {
            bail!(
                "Tencent API error: status={}, message={}",
                resp.status,
                resp.message.as_deref().unwrap_or("unknown")
            );
        }

        let result = resp.result.ok_or_else(|| anyhow::anyhow!("Tencent: empty result"))?;
        // let ad = result.ad_info;
        let raw = serde_json::to_value(&result)?;

        debug!(address = %result.address, "Tencent geocode OK");

        Ok(GeocodeResult {
            display_name: result.formatted_addresses.recommend.clone(),
            city: Some(result.address_component.city.unwrap_or_default()),
            county: None,
            province: Some(result.address_component.province.unwrap_or_default()),
            country: Some(result.address_component.nation.unwrap_or_default()),
            postcode: Some(result.ad_info.adcode.unwrap_or_default()),
            name: Some(result.formatted_addresses.recommend.clone()),
            house_number: Some(result.address_component.street_number.unwrap_or_default()),
            road: Some(result.address_component.street.unwrap_or_default()),
            neighbourhood: None,
            latitude: Some(result.location.lat),
            longitude: Some(result.location.lng),
            raw: raw,
            state_district: Some(result.address_component.district.unwrap_or_default()),
        })
    }
}

// ---- Response types ----

#[derive(Deserialize)]
struct TencentResponse {
    status: i32,
    message: Option<String>,
    result: Option<TencentResult>,
}

#[derive(Deserialize, Serialize)]
struct TencentResult {
    address: String,
    ad_info: TencentAdInfo,
    location: TencentLocation,
    address_component: TencentAddressComponent,
    formatted_addresses: TencentFormattedAddresses,
}

#[derive(Debug, Deserialize, Serialize)]
struct TencentAdInfo {
    adcode: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct TencentLocation {
    lat: f64,
    lng: f64,
}

#[derive(Debug, Deserialize, Serialize)]
struct TencentAddressComponent {
    nation: Option<String>,
    province: Option<String>,
    city: Option<String>,
    district: Option<String>,
    street: Option<String>,
    street_number: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct TencentFormattedAddresses {
    recommend: String,
}
