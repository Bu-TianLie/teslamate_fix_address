-- ============================================================
-- TeslaMate Geocoder - Database Migration
-- ============================================================
-- Run against the TeslaMate PostgreSQL database.
-- Usage: psql -d teslamate -f sql/migrations/001_init.sql
-- ============================================================

BEGIN;

-- -------------------------------------------------------
-- 1. geocode_queue 表
-- -------------------------------------------------------
CREATE TABLE IF NOT EXISTS geocode_queue (
    id            BIGSERIAL    PRIMARY KEY,
    drive_id      INT4       NOT NULL,
    address_type  VARCHAR(10)  NOT NULL CHECK (address_type IN ('start', 'end')),
    latitude      NUMERIC(8,6) NOT NULL,
    longitude     NUMERIC(9,6) NOT NULL,
    status        VARCHAR(20)  NOT NULL DEFAULT 'pending'
                  CHECK (status IN ('pending', 'processing', 'done', 'failed')),
    retries       INTEGER      NOT NULL DEFAULT 0,
    error_msg     TEXT,
    created_at    TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

-- -------------------------------------------------------
-- 2. 索引
-- -------------------------------------------------------
-- 核心队列查询索引：快速取 pending 条目
CREATE INDEX IF NOT EXISTS idx_geocode_queue_status_created
    ON geocode_queue (status, created_at)
    WHERE status IN ('pending', 'failed');

-- 坐标去重索引：避免重复入队
CREATE INDEX IF NOT EXISTS idx_geocode_queue_lat_lng
    ON geocode_queue (latitude, longitude);

-- drive_id 查找索引
CREATE INDEX IF NOT EXISTS idx_geocode_queue_drive_id
    ON geocode_queue (drive_id);

-- -------------------------------------------------------
-- 3. addresses 表坐标去重索引
-- -------------------------------------------------------
-- 如果 TeslaMate 的 addresses 表没有此索引则创建
CREATE UNIQUE INDEX IF NOT EXISTS idx_addresses_lat_lng_unique
    ON addresses (latitude, longitude);

-- -------------------------------------------------------
-- 4. updated_at 自动更新触发器
-- -------------------------------------------------------
CREATE OR REPLACE FUNCTION update_geocode_queue_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_geocode_queue_updated_at ON geocode_queue;
CREATE TRIGGER trg_geocode_queue_updated_at
    BEFORE UPDATE ON geocode_queue
    FOR EACH ROW
    EXECUTE FUNCTION update_geocode_queue_updated_at();

-- -------------------------------------------------------
-- 5. 自动入队触发器（可选，推荐）
--    当 drives 表更新记录且缺少地址时自动入队;drivers 表在行程开始时即创建记录，但地址信息可能在行程结束后才更新，因此此触发器可确保及时入队
-- -------------------------------------------------------
CREATE OR REPLACE FUNCTION auto_enqueue_geocode()
RETURNS TRIGGER AS $$
BEGIN
    -- start address 缺失
    IF NEW.start_address_id IS NULL THEN
        INSERT INTO geocode_queue (drive_id, address_type, latitude, longitude, status)
        SELECT NEW.id, 'start', p.latitude, p.longitude, 'pending'
        FROM positions p
        WHERE p.id = NEW.start_position_id
          AND p.latitude IS NOT NULL
          AND p.longitude IS NOT NULL
        ON CONFLICT DO NOTHING;
    END IF;

    -- end address 缺失
    IF NEW.end_address_id IS NULL THEN
        INSERT INTO geocode_queue (drive_id, address_type, latitude, longitude, status)
        SELECT NEW.id, 'end', p.latitude, p.longitude, 'pending'
        FROM positions p
        WHERE p.id = NEW.end_position_id
          AND p.latitude IS NOT NULL
          AND p.longitude IS NOT NULL
        ON CONFLICT DO NOTHING;
    END IF;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_auto_enqueue_geocode ON drives;
CREATE TRIGGER trg_auto_enqueue_geocode
    AFTER UPDATE ON drives
    FOR EACH ROW
    EXECUTE FUNCTION auto_enqueue_geocode();

COMMIT;
