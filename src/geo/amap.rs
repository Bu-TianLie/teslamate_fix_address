use anyhow::{bail, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::debug;

use super::coord::wgs84_to_gcj02;
use super::provider::{CoordSystem, GeocodeResult, GeoProvider};

pub struct AmapProvider {
    key: String,
    client: Client,
}

impl AmapProvider {
    pub fn new(key: String) -> Self {
        Self {
            key,
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("failed to build reqwest client"),
        }
    }
}

#[async_trait]
impl GeoProvider for AmapProvider {
    fn name(&self) -> &str {
        "amap"
    }

    fn coord_system(&self) -> CoordSystem {
        CoordSystem::Gcj02
    }

    async fn reverse_geocode(&self, lat: f64, lng: f64) -> Result<GeocodeResult> {
        let (glat, glng) = wgs84_to_gcj02(lat, lng);
        let location = format!("{:.6},{:.6}", glng, glat); // Amap: lng,lat

        let resp: AmapResponse = self
            .client
            .get("https://restapi.amap.com/v3/geocode/regeo")
            .query(&[
                ("location", location.as_str()),
                ("key", self.key.as_str()),
                ("extensions", "base"),
                ("coordsys", "autonavi"),
            ])
            .send()
            .await?
            .json()
            .await?;

        if resp.status != "1" {
            bail!(
                "Amap API error: status={}, info={}",
                resp.status,
                resp.info.as_deref().unwrap_or("unknown")
            );
        }

        let regeocode = resp.regeocode.ok_or_else(|| anyhow::anyhow!("Amap: empty result"))?;
        let raw: serde_json::Value = serde_json::to_value(&regeocode)?;
        let comp = regeocode.address_component;

        debug!(address = %regeocode.formatted_address, "Amap geocode OK");

        Ok(GeocodeResult {
            display_name: regeocode.formatted_address,
            city: comp.city,
            county: comp.district,
            province: comp.province,
            country: Some("中国".into()),
            postcode: comp.citycode,
            name: None,
            house_number: comp.street_number.as_ref().and_then(|sn| sn.number.clone()),
            road: comp.street_number.as_ref().and_then(|sn| sn.street.clone()),
            neighbourhood: comp.neighborhood.as_ref().and_then(|n| n.name.clone()),
            latitude: None,
            longitude: None,
            raw: raw,
            state_district: None,
            // town: comp.township,
            // village: None,
        })
    }
}

// ---- Response types ----

#[derive(Deserialize)]
struct AmapResponse {
    status: String,
    info: Option<String>,
    regeocode: Option<AmapRegeocode>,
}

#[derive(Deserialize, Serialize)]
struct AmapRegeocode {
    formatted_address: String,
    address_component: AmapAddressComponent,
}

#[derive(Deserialize, Serialize)]
struct AmapAddressComponent {
    province: Option<String>,
    city: Option<String>,
    district: Option<String>,
    township: Option<String>,
    citycode: Option<String>,
    street_number: Option<AmapStreetNumber>,
    neighborhood: Option<AmapNeighborhood>,
}

#[derive(Deserialize, Serialize)]
struct AmapStreetNumber {
    street: Option<String>,
    number: Option<String>,
}

#[derive(Deserialize, Serialize)]
struct AmapNeighborhood {
    name: Option<String>,
}
