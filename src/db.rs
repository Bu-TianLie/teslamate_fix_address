use anyhow::Result;
use sqlx::PgPool;
use tracing::{debug, info};

// ================================================================
// Helper: round f64 to 6 decimal places (numeric(8,6) precision)
// ================================================================

fn round_coord(v: f64) -> f64 {
    (v * 1_000_000.0).round() / 1_000_000.0
}

// ================================================================
// Schema bootstrap (idempotent)
// ================================================================

pub async fn ensure_schema(pool: &PgPool) -> Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS geocode_queue (
            id            BIGSERIAL PRIMARY KEY,
            drive_id      int4      NOT NULL,
            address_type  VARCHAR(10) NOT NULL CHECK (address_type IN ('start','end')),
            latitude      NUMERIC(8,6) NOT NULL,
            longitude     NUMERIC(9,6) NOT NULL,
            status        VARCHAR(20) NOT NULL DEFAULT 'pending'
                          CHECK (status IN ('pending','processing','done','failed')),
            retries       INTEGER     NOT NULL DEFAULT 0,
            error_msg     TEXT,
            created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )
        "#,
    )
    .execute(pool)
    .await?;

    let indices = [
        "CREATE INDEX IF NOT EXISTS idx_geocode_queue_status_created ON geocode_queue (status, created_at) WHERE status IN ('pending','failed')",
        "CREATE INDEX IF NOT EXISTS idx_geocode_queue_lat_lng ON geocode_queue (latitude, longitude)",
        "CREATE INDEX IF NOT EXISTS idx_geocode_queue_drive_id ON geocode_queue (drive_id)",
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_addresses_lat_lng_unique ON addresses (latitude, longitude)",
    ];
    for sql in &indices {
        sqlx::query(sql).execute(pool).await?;
    }

    // updated_at trigger
    sqlx::query(
        r#"
        CREATE OR REPLACE FUNCTION update_geocode_queue_updated_at()
        RETURNS TRIGGER AS $$
        BEGIN NEW.updated_at = NOW(); RETURN NEW; END;
        $$ LANGUAGE plpgsql
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        DO $$ BEGIN
          IF NOT EXISTS (
            SELECT 1 FROM pg_trigger WHERE tgname = 'trg_geocode_queue_updated_at'
          ) THEN
            CREATE TRIGGER trg_geocode_queue_updated_at
              BEFORE UPDATE ON geocode_queue
              FOR EACH ROW EXECUTE FUNCTION update_geocode_queue_updated_at();
          END IF;
        END $$;
        "#,
    )
    .execute(pool)
    .await?;

    info!("Database schema ensured");
    Ok(())
}

// ================================================================
// Queue item
// ================================================================

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct QueueItem {
    pub id: i64,
    pub drive_id: i32,
    pub address_type: String,
    pub latitude: f64,
    pub longitude: f64,
    pub retries: i32,
}

// ================================================================
// Scanning — find drives missing addresses
// ================================================================

pub async fn scan_missing_addresses(pool: &PgPool) -> Result<Vec<(i32, String, f64, f64)>> {
    let rows: Vec<(i32, String, f64, f64)> = sqlx::query_as::<_, (i32, String, f64, f64)>(
        r#"
        SELECT d.id, 'start'::text,
               sp.latitude::double precision, sp.longitude::double precision
        FROM drives d
        JOIN positions sp ON sp.id = d.start_position_id
        WHERE d.start_address_id IS NULL
          AND sp.latitude IS NOT NULL AND sp.longitude IS NOT NULL

        UNION ALL

        SELECT d.id, 'end'::text,
               ep.latitude::double precision, ep.longitude::double precision
        FROM drives d
        JOIN positions ep ON ep.id = d.end_position_id
        WHERE d.end_address_id IS NULL
          AND ep.latitude IS NOT NULL AND ep.longitude IS NOT NULL
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

// ================================================================
// Enqueue
// ================================================================

pub async fn enqueue_items(pool: &PgPool, items: &[(i32, String, f64, f64)]) -> Result<u64> {
    debug!(count = items.len(), "Enqueuing items");
    if items.is_empty() {
        return Ok(0);
    }

    let mut tx = pool.begin().await?;
    let mut count: u64 = 0;

    for (drive_id, addr_type, lat, lng) in items {
        let lat_r = round_coord(*lat);
        let lng_r = round_coord(*lng);
        debug!(drive_id, addr_type, lat = lat_r, lng = lng_r, "Enqueueing item");
        let res = sqlx::query(
            r#"
            INSERT INTO geocode_queue (drive_id, address_type, latitude, longitude, status)
            SELECT $1, $2, $3::numeric(8,6), $4::numeric(9,6), 'pending'
            WHERE NOT EXISTS (
                SELECT 1 FROM geocode_queue
                WHERE drive_id = $1 AND address_type = $2 AND status IN ('pending','processing')
            )

            "#,
        )
        .bind(drive_id)
        .bind(addr_type)
        .bind(lat_r)
        .bind(lng_r)
        .execute(&mut *tx)
        .await?;

        count += res.rows_affected();
    }

    tx.commit().await?;

    if count > 0 {
        debug!(count, "Enqueued new items");
    }
    Ok(count)
}

// ================================================================
// Fetch batch (FOR UPDATE SKIP LOCKED)
// ================================================================

pub async fn fetch_batch(pool: &PgPool, batch_size: i64) -> Result<Vec<QueueItem>> {
    let items = sqlx::query_as::<_, QueueItem>(
        r#"
        UPDATE geocode_queue
        SET status = 'processing', updated_at = NOW()
        WHERE id = ANY(
            SELECT id FROM geocode_queue
            WHERE status IN ('pending', 'failed')
              AND retries < 10
            ORDER BY created_at
            LIMIT $1
            FOR UPDATE SKIP LOCKED
        )
        RETURNING id, drive_id, address_type,
                  latitude::double precision,
                  longitude::double precision,
                  retries
        "#,
    )
    .bind(batch_size)
    .fetch_all(pool)
    .await?;

    Ok(items)
}

// ================================================================
// Address lookup / insert
// ================================================================

pub async fn find_address_by_coord(pool: &PgPool, lat: f64, lng: f64) -> Result<Option<i32>> {
    let lat_r = round_coord(lat);
    let lng_r = round_coord(lng);

    let result = sqlx::query_scalar::<_, i32>(
        r#"
        SELECT id FROM addresses
        WHERE latitude = $1::numeric(8,6) AND longitude = $2::numeric(9,6)
        LIMIT 1
        "#,
    )
    .bind(lat_r)
    .bind(lng_r)
    .fetch_optional(pool)
    .await?;

    Ok(result)
}

pub async fn insert_address(
    pool: &PgPool,
    lat: f64,
    lng: f64,
    display_name: &str,
    city: Option<&str>,
    // county: Option<&str>,
    state: Option<&str>,
    country: Option<&str>,
    postcode: Option<&str>,
    name: Option<&str>,
    house_number: Option<&str>,
    road: Option<&str>,
    neighbourhood: Option<&str>,
    state_district: Option<&str>,
    raw: serde_json::Value,
    // town: Option<&str>,
    // village: Option<&str>,
) -> Result<i32> {
    let lat_r = round_coord(lat);
    let lng_r = round_coord(lng);

    let id = sqlx::query_scalar::<_, i32>(
        r#"
        INSERT INTO addresses
            (name, house_number, road, neighbourhood, city,
             state, postcode, country, display_name, latitude, longitude, state_district,raw, inserted_at, updated_at)
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,
                $10::numeric(8,6), $11::numeric(9,6), $12,$13, NOW(), NOW())
        ON CONFLICT (latitude, longitude)
        DO UPDATE SET display_name = EXCLUDED.display_name
        RETURNING id
        "#,
    )
    .bind(name)
    .bind(house_number)
    .bind(road)
    .bind(neighbourhood)
    // .bind(village)
    // .bind(town)
    .bind(city)
    .bind(state)
    .bind(postcode)
    .bind(country)
    .bind(display_name)
    .bind(lat_r)
    .bind(lng_r)
    .bind(state_district)
    .bind(raw)
    .fetch_one(pool)
    .await?;

    Ok(id)
}

// ================================================================
// Update drive address
// ================================================================

pub async fn update_drive_address(
    pool: &PgPool,
    drive_id: i32,
    address_type: &str,
    address_id: i32,
) -> Result<()> {
    let sql = match address_type {
        "start" => "UPDATE drives SET start_address_id = $1 WHERE id = $2",
        "end" => "UPDATE drives SET end_address_id = $1 WHERE id = $2",
        _ => anyhow::bail!("Invalid address_type: {}", address_type),
    };

    sqlx::query(sql)
        .bind(address_id)
        .bind(drive_id)
        .execute(pool)
        .await?;

    Ok(())
}

// ================================================================
// charge miss affress
// ================================================================

pub async fn update_charge_address(
    pool: &PgPool,

) -> Result<()> {
    let sql = r#"
                    UPDATE charging_processes cp
                    SET
                        address_id = a.id
                    FROM addresses a ,positions p
                    WHERE cp.address_id IS NULL
                    AND p.id = cp.position_id
                    and a.latitude = p.latitude 
                    and a.longitude =p.longitude;
                    "#;
    sqlx::query(sql )
        .execute(pool)
        .await?;
    Ok(())  
}

// ================================================================
// Queue status updates
// ================================================================

pub async fn mark_done(pool: &PgPool, id: i64) -> Result<()> {
    sqlx::query("UPDATE geocode_queue SET status = 'done', error_msg = NULL WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn mark_dead(pool: &PgPool, id: i64, error: &str) -> Result<()> {
    sqlx::query(
        "UPDATE geocode_queue SET status = 'failed', error_msg = $2 WHERE id = $1",
    )
    .bind(id)
    .bind(format!("[DEAD] {}", error))
    .execute(pool)
    .await?;
    Ok(())
}

