# TeslaMate Geocoder

解决 TeslaMate 在中国大陆无法通过 OpenStreetMap 进行地址反向解析的问题。

本服务监控 TeslaMate 数据库中地址为空的行程和充电记录，使用国内地图服务（腾讯地图、高德地图、百度地图）自动补全地址信息，写回 TeslaMate 原有的 `addresses` 和 `drives` 表中。

## 特性

- **多 Provider 自动降级** — 按优先级依次调用腾讯/高德/百度，单个失败自动切换
- **坐标自动转换** — 自动处理 WGS84、GCJ-02、BD-09 坐标系间的转换
- **增量 + 全量** — 支持持续监听新行程，也支持一次性补全历史数据 (`--backfill`)
- **数据库级触发器** — 通过 PostgreSQL 触发器自动捕获地址缺失的记录
- **内存 + DB 双重缓存** — 避免对相同坐标的重复 API 调用
- **限流 & 重试** — 内置 QPS 限流和指数退避重试机制
- **Prometheus 监控** — 暴露 `/metrics` 端点，包含成功率、失败率、延迟等指标

## 工作原理

```
TeslaMate DB (drives / charging_processes)
        │
        ▼
  geocode_queue (触发器自动入队)
        │
        ▼
  GeocoderWorker (轮询批次)
        │
        ├── 缓存命中？→ 直接写回地址 ID
        │
        ▼
  地图 API 调用链 (Tencent → Amap → Baidu)
        │
        ▼
  写入 addresses 表 → 更新 drives / charging_processes
```

1. 启动时扫描 `drives` 表中 `start_address_id` 或 `end_address_id` 为空的记录，插入队列
2. PostgreSQL 触发器持续监听后续更新，自动入队新产生的空地址记录
3. Worker 以批次（默认 10 条）从队列中取任务，先查缓存，未命中则调用地图 API
4. 按配置的优先级依次尝试各 Provider，全部失败则标记为死信
5. 失败任务（`retries < 10`）会在后续轮询中自动重试

## 快速开始

### 1. 配置环境变量

```bash
cp .env.example .env
```

编辑 `.env`，填入数据库连接和至少一个地图 API Key：

```env
DATABASE_URL=postgres://user:pass@host:5432/teslamate

# 至少配置一个
TENCENT_MAP_KEY=your_tencent_key
AMAP_KEY=your_amap_key
BAIDU_AK=your_baidu_key
```

### 2. 构建 & 运行

```bash
# 本地构建
cargo build --release

# 运行
./target/release/teslamate-geocoder \
    --batch-size 10 \
    --qps 3 \
    --metrics
```

### 3. Docker 部署

```bash
docker compose up -d
```

默认启动参数：`--batch-size 10 --qps 3 --metrics`，Prometheus 端口 `9090`。

### 4. systemd 部署（裸机）

```bash
cp systemd/teslamate-geocoder.service /etc/systemd/system/
systemctl daemon-reload
systemctl enable --now teslamate-geocoder
```

服务以 `teslamate` 用户运行，通过 `EnvironmentFile` 加载 `.env`。

## 配置项

### 环境变量

| 变量 | 说明 | 默认值 |
|------|------|--------|
| `DATABASE_URL` | PostgreSQL 连接串 | 必填 |
| `TENCENT_MAP_KEY` | 腾讯地图 API Key | — |
| `AMAP_KEY` | 高德地图 API Key | — |
| `BAIDU_AK` | 百度地图 API Key | — |
| `PROVIDER_ORDER` | Provider 优先级（逗号分隔） | `tencent,amap,baidu` |
| `MAX_RETRIES` | 单条记录最大重试次数 | `3` |
| `SCAN_INTERVAL_SECS` | 队列空时的轮询间隔（秒） | `30` |
| `DB_MAX_CONNECTIONS` | 数据库连接池大小 | `5` |
| `RUST_LOG` | 日志级别 | `teslamate_geocoder=info` |

### CLI 参数

| 参数 | 说明 | 默认值 |
|------|------|--------|
| `--batch-size` | 每批处理条数 | `10` |
| `--qps` | API 请求 QPS 上限 | `3` |
| `--provider` | 指定单个 Provider | — |
| `--dry-run` | 只读模式，不写入数据库 | — |
| `--metrics` | 启用 Prometheus 指标 | — |
| `--metrics-addr` | 指标监听地址 | `0.0.0.0:9090` |
| `--backfill` | 一次性补全模式（处理完退出） | — |

## 监控

服务默认暴露以下端点：

- `GET /metrics` — Prometheus 指标（`geocode_success_total`, `geocode_failure_total`, `geocode_latency_seconds{provider}`）
- `GET /health` — 健康检查
- `GET /healthz` — 健康检查（别名）

```bash
curl http://localhost:9090/metrics
```

## 支持的 Provider

| Provider | API | 坐标系 | 说明 |
|----------|-----|--------|------|
| 腾讯地图 | `apis.map.qq.com` | GCJ-02 | 支持直接传入 WGS84 (GPS) 坐标 |
| 高德地图 | `restapi.amap.com` | GCJ-02 | 需要 WGS84→GCJ-02 转换 |
| 百度地图 | `api.map.baidu.com` | BD-09 | 需要 WGS84→GCJ-02→BD-09 转换 |

中国境外坐标不做转换，直接传入各 Provider。

## 项目结构

```
src/
├── main.rs              # 入口：CLI 解析、启动、主循环
├── config.rs            # 配置读取（环境变量）
├── db.rs                # 数据库操作（队列、地址、行程）
├── worker.rs            # 核心处理循环
├── geo/
│   ├── provider.rs      # GeoProvider trait + GeocodeResult
│   ├── tencent.rs       # 腾讯地图实现
│   ├── amap.rs          # 高德地图实现
│   ├── baidu.rs         # 百度地图实现
│   ├── coord.rs         # 坐标系转换 (WGS84/GCJ-02/BD-09)
│   └── geohash.rs       # GeoHash 编解码
└── util/
    ├── cache.rs         # 内存坐标缓存
    ├── limiter.rs       # Token-bucket 限流器
    ├── metrics.rs       # Prometheus 指标服务
    └── retry.rs         # 指数退避重试
migrations/
└── 001_init.sql         # 队列表 + 触发器
systemd/
└── teslamate-geocoder.service
```

## License

MIT
