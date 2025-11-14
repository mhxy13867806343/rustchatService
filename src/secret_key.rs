use crate::errors::DomainError;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha512};
use sqlx::{PgPool, Postgres};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

// 配置常量
const TEMP_KEY_EXPIRY_MINUTES: i64 = 3; // 临时密钥有效期（分钟）
const TEMP_KEY_LENGTH: usize = 128; // 临时密钥长度（位）
const WS_KEY_LENGTH: usize = 64; // WebSocket 密钥长度（位）

// 临时操作密钥类型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TempKeyType {
    FileDownload,  // 文件下载
    FileUpload,    // 文件上传
    ApiAccess,     // API 访问
    DataExport,    // 数据导出
}

// 临时密钥信息
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct TempSecretKey {
    pub id: i64,
    pub key_value: String,           // 128位密钥（加密存储）
    pub key_hash: String,            // 密钥哈希（用于查询）
    pub user_id: i64,                // 用户ID
    pub key_type: String,            // 密钥类型
    pub used: bool,                  // 是否已使用
    pub used_at: Option<DateTime<Utc>>, // 使用时间
    pub expires_at: DateTime<Utc>,  // 过期时间
    pub created_at: DateTime<Utc>,  // 创建时间
    pub metadata: Option<String>,    // 元数据（JSON）
}

// WebSocket 会话密钥
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketKey {
    pub key_value: String,           // 密钥值
    pub user_id: i64,                // 用户ID
    pub conversation_id: i64,        // 会话ID
    pub connected_at: DateTime<Utc>, // 连接时间
    pub last_active: DateTime<Utc>, // 最后活跃时间
}

// 密钥服务
#[derive(Clone)]
pub struct SecretKeyService {
    pub pool: PgPool,
    // WebSocket 密钥缓存：key_value -> WebSocketKey
    pub ws_keys: Arc<RwLock<HashMap<String, WebSocketKey>>>,
    // 用户当前使用的临时密钥：user_id -> key_hash
    pub active_temp_keys: Arc<RwLock<HashMap<i64, String>>>,
}

impl SecretKeyService {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            ws_keys: Arc::new(RwLock::new(HashMap::new())),
            active_temp_keys: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    // ==================== 临时操作密钥 ====================

    /// 生成临时操作密钥
    /// 
    /// 参数：
    /// - user_id: 用户ID
    /// - username: 用户名
    /// - user_agent: 浏览器 User-Agent
    /// - key_type: 密钥类型
    /// - metadata: 可选的元数据
    pub async fn generate_temp_key(
        &self,
        user_id: i64,
        username: &str,
        user_agent: &str,
        key_type: TempKeyType,
        metadata: Option<String>,
    ) -> Result<String, DomainError> {
        // 1. 检查用户是否有正在使用的密钥
        let active_keys = self.active_temp_keys.read().await;
        if let Some(existing_key_hash) = active_keys.get(&user_id) {
            // 检查是否还有效
            let is_valid = self.check_temp_key_valid(existing_key_hash).await?;
            if is_valid {
                return Err(DomainError::Validation(
                    "您有一个正在使用的密钥，请等待其过期或使用完毕".into()
                ));
            }
        }
        drop(active_keys);

        // 2. 生成密钥组件
        let timestamp = Utc::now().timestamp_millis();
        let random_36 = Uuid::new_v4().to_string().replace("-", ""); // 36位随机字符
        
        // 3. 组合生成原始字符串
        let raw_string = format!(
            "{}|{}|{}|{}|{}",
            user_id,
            username,
            timestamp,
            random_36,
            user_agent
        );

        // 4. 使用 SHA-512 生成 128 位密钥（取前128位）
        let mut hasher = Sha512::new();
        hasher.update(raw_string.as_bytes());
        let result = hasher.finalize();
        let key_value = hex::encode(&result[..64]); // 64字节 = 512位，取前64字节 = 128位十六进制

        // 5. 生成密钥哈希（用于数据库查询）
        let key_hash = self.hash_key(&key_value);

        // 6. 计算过期时间
        let expires_at = Utc::now() + Duration::minutes(TEMP_KEY_EXPIRY_MINUTES);

        // 7. 存储到数据库
        let key_type_str = match key_type {
            TempKeyType::FileDownload => "file_download",
            TempKeyType::FileUpload => "file_upload",
            TempKeyType::ApiAccess => "api_access",
            TempKeyType::DataExport => "data_export",
        };

        sqlx::query(
            r#"INSERT INTO temp_secret_keys 
               (key_value, key_hash, user_id, key_type, expires_at, metadata)
               VALUES ($1, $2, $3, $4, $5, $6)"#
        )
        .bind(&key_value)
        .bind(&key_hash)
        .bind(user_id)
        .bind(key_type_str)
        .bind(expires_at)
        .bind(&metadata)
        .execute(&self.pool)
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        // 8. 记录到活跃密钥
        let mut active_keys = self.active_temp_keys.write().await;
        active_keys.insert(user_id, key_hash);

        Ok(key_value)
    }

    /// 验证并使用临时密钥
    /// 
    /// 返回：(user_id, metadata)
    pub async fn validate_and_use_temp_key(
        &self,
        key_value: &str,
        requesting_user_id: i64,
    ) -> Result<(i64, Option<String>), DomainError> {
        let key_hash = self.hash_key(key_value);

        // 1. 开启事务
        let mut tx = self.pool.begin().await.map_err(|e| DomainError::Db(e.to_string()))?;

        // 2. 查询密钥（加锁）
        let key_info = sqlx::query_as::<Postgres, TempSecretKey>(
            r#"SELECT * FROM temp_secret_keys 
               WHERE key_hash = $1 
               FOR UPDATE"#
        )
        .bind(&key_hash)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        let key = match key_info {
            Some(k) => k,
            None => return Err(DomainError::NotFound),
        };

        // 3. 检查是否已过期
        if Utc::now() > key.expires_at {
            // 删除过期密钥
            sqlx::query("DELETE FROM temp_secret_keys WHERE id = $1")
                .bind(key.id)
                .execute(&mut *tx)
                .await
                .map_err(|e| DomainError::Db(e.to_string()))?;
            
            tx.commit().await.map_err(|e| DomainError::Db(e.to_string()))?;
            
            return Err(DomainError::Gone);
        }

        // 4. 检查是否已使用
        if key.used {
            return Err(DomainError::Validation("密钥已被使用".into()));
        }

        // 5. 检查用户权限（只有创建者可以使用）
        if key.user_id != requesting_user_id {
            return Err(DomainError::Validation("此密钥仅限创建用户使用".into()));
        }

        // 6. 标记为已使用
        sqlx::query(
            r#"UPDATE temp_secret_keys 
               SET used = true, used_at = NOW() 
               WHERE id = $1"#
        )
        .bind(key.id)
        .execute(&mut *tx)
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        // 7. 提交事务
        tx.commit().await.map_err(|e| DomainError::Db(e.to_string()))?;

        // 8. 从活跃密钥中移除
        let mut active_keys = self.active_temp_keys.write().await;
        active_keys.remove(&key.user_id);

        Ok((key.user_id, key.metadata))
    }

    /// 检查密钥是否有效
    async fn check_temp_key_valid(&self, key_hash: &str) -> Result<bool, DomainError> {
        let result: Option<(bool, DateTime<Utc>)> = sqlx::query_as(
            r#"SELECT used, expires_at FROM temp_secret_keys WHERE key_hash = $1"#
        )
        .bind(key_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        match result {
            Some((used, expires_at)) => {
                Ok(!used && Utc::now() <= expires_at)
            }
            None => Ok(false),
        }
    }

    /// 清理过期的临时密钥
    pub async fn cleanup_expired_temp_keys(&self) -> Result<u64, DomainError> {
        let result = sqlx::query(
            r#"DELETE FROM temp_secret_keys WHERE expires_at < NOW()"#
        )
        .execute(&self.pool)
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        Ok(result.rows_affected())
    }

    // ==================== WebSocket 会话密钥 ====================

    /// 生成 WebSocket 会话密钥
    /// 
    /// 每个聊天会话一个密钥，连接期间有效
    pub async fn generate_ws_key(
        &self,
        user_id: i64,
        conversation_id: i64,
    ) -> Result<String, DomainError> {
        // 1. 检查是否已有该会话的密钥
        let ws_keys = self.ws_keys.read().await;
        for (key, info) in ws_keys.iter() {
            if info.user_id == user_id && info.conversation_id == conversation_id {
                // 已存在，返回现有密钥
                return Ok(key.clone());
            }
        }
        drop(ws_keys);

        // 2. 生成新密钥
        let timestamp = Utc::now().timestamp_millis();
        let random = Uuid::new_v4().to_string().replace("-", "");
        
        let raw_string = format!(
            "ws|{}|{}|{}|{}",
            user_id,
            conversation_id,
            timestamp,
            random
        );

        let mut hasher = Sha512::new();
        hasher.update(raw_string.as_bytes());
        let result = hasher.finalize();
        let key_value = hex::encode(&result[..32]); // 32字节 = 64位十六进制

        // 3. 存储到内存
        let ws_key = WebSocketKey {
            key_value: key_value.clone(),
            user_id,
            conversation_id,
            connected_at: Utc::now(),
            last_active: Utc::now(),
        };

        let mut ws_keys = self.ws_keys.write().await;
        ws_keys.insert(key_value.clone(), ws_key);

        Ok(key_value)
    }

    /// 验证 WebSocket 密钥
    pub async fn validate_ws_key(&self, key_value: &str) -> Result<(i64, i64), DomainError> {
        let mut ws_keys = self.ws_keys.write().await;
        
        match ws_keys.get_mut(key_value) {
            Some(key) => {
                // 更新最后活跃时间
                key.last_active = Utc::now();
                Ok((key.user_id, key.conversation_id))
            }
            None => Err(DomainError::NotFound),
        }
    }

    /// 移除 WebSocket 密钥（连接断开时）
    pub async fn remove_ws_key(&self, key_value: &str) -> Result<(), DomainError> {
        let mut ws_keys = self.ws_keys.write().await;
        ws_keys.remove(key_value);
        Ok(())
    }

    /// 获取用户的所有 WebSocket 密钥
    pub async fn get_user_ws_keys(&self, user_id: i64) -> Vec<String> {
        let ws_keys = self.ws_keys.read().await;
        ws_keys
            .iter()
            .filter(|(_, info)| info.user_id == user_id)
            .map(|(key, _)| key.clone())
            .collect()
    }

    // ==================== 辅助方法 ====================

    /// 生成密钥哈希
    fn hash_key(&self, key_value: &str) -> String {
        let mut hasher = Sha512::new();
        hasher.update(key_value.as_bytes());
        hex::encode(hasher.finalize())
    }

    /// 混淆密钥显示（双击时显示为乱码）
    pub fn obfuscate_key(key_value: &str) -> String {
        // 将密钥转换为看起来像乱码的字符
        key_value
            .chars()
            .map(|c| {
                match c {
                    '0'..='9' => char::from_u32(0x2460 + (c as u32 - '0' as u32)).unwrap_or('�'),
                    'a'..='f' => char::from_u32(0x24B6 + (c as u32 - 'a' as u32)).unwrap_or('�'),
                    _ => '�',
                }
            })
            .collect()
    }
}
