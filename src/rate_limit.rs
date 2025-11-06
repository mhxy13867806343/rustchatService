use redis::AsyncCommands;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct RateLimiter {
    pub redis: redis::Client,
}

impl RateLimiter {
    pub fn new(redis_url: &str) -> Result<Self, redis::RedisError> {
        let client = redis::Client::open(redis_url)?;
        Ok(Self { redis: client })
    }

    // 简单令牌桶：用户/资源维度，每秒N个令牌
    pub async fn check_and_consume(&self, key: &str, capacity: i64, refill_per_sec: i64) -> Result<bool, redis::RedisError> {
        let mut conn = self.redis.get_async_connection().await?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
        let bucket_key = format!("rl:{}", key);

        // 使用lua保证原子性（但此处简化为多步操作）
        let last_ts: i64 = conn.hget(&bucket_key, "ts").await.unwrap_or(0);
        let mut tokens: i64 = conn.hget(&bucket_key, "tokens").await.unwrap_or(capacity);

        if last_ts == 0 {
            conn.hset_multiple::<_, _, _, ()>(&bucket_key, &[("ts", now), ("tokens", capacity)]).await?;
        } else {
            let elapsed = now - last_ts;
            let refill = elapsed * refill_per_sec;
            tokens = (tokens + refill).min(capacity);
            conn.hset_multiple::<_, _, _, ()>(&bucket_key, &[("ts", now), ("tokens", tokens)]).await?;
        }

        if tokens > 0 {
            tokens -= 1;
            conn.hset::<_, _, _, ()>(&bucket_key, "tokens", tokens).await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}