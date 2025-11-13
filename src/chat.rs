use crate::errors::DomainError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Transaction};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

// 配置常量
const MESSAGE_INTERVAL_SECONDS: i64 = 1; // 连续发送消息最小间隔（秒）
const LOCK_TIMEOUT_SECONDS: i64 = 10; // 锁超时时间（秒）
const TRANSACTION_TIMEOUT: Duration = Duration::from_secs(30); // 事务超时时间
const MAX_MESSAGE_LENGTH: usize = 5000; // 最大消息长度
const MAX_GROUP_MEMBERS: usize = 500; // 最大群成员数
const MAX_FILE_SIZE: i64 = 10 * 1024 * 1024; // 最大文件大小 10MB

// 消息类型
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "message_type", rename_all = "lowercase")]
pub enum MessageType {
    Text,      // 文本消息
    Image,     // 图片
    File,      // 文件
    Voice,     // 语音
    Video,     // 视频
    System,    // 系统消息
}

// 会话类型
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "conversation_type", rename_all = "lowercase")]
pub enum ConversationType {
    Private,   // 一对一私聊
    Group,     // 群聊
}

// 消息状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageStatus {
    Sent,      // 已发送
    Delivered, // 已送达
    Read,      // 已读
}

// 在线用户信息
#[derive(Debug, Clone)]
pub struct OnlineUser {
    pub user_id: i64,
    pub username: String,
    pub connected_at: DateTime<Utc>,
}

// 消息结构
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Message {
    pub id: i64,
    pub conversation_id: i64,
    pub sender_id: i64,
    pub message_type: MessageType,
    pub content: String,           // 文本内容或文件URL
    pub file_url: Option<String>,  // 文件/图片URL
    pub file_name: Option<String>, // 文件名
    pub file_size: Option<i64>,    // 文件大小（字节）
    pub created_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

// 会话结构
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Conversation {
    pub id: i64,
    pub conversation_type: ConversationType,
    pub name: Option<String>,      // 群聊名称
    pub avatar: Option<String>,    // 群聊头像
    pub owner_id: Option<i64>,     // 群主ID
    pub created_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

// 会话成员
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ConversationMember {
    pub id: i64,
    pub conversation_id: i64,
    pub user_id: i64,
    pub joined_at: DateTime<Utc>,
    pub left_at: Option<DateTime<Utc>>,
}

// 离线消息
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct OfflineMessage {
    pub id: i64,
    pub user_id: i64,              // 接收者ID
    pub message_id: i64,           // 消息ID
    pub created_at: DateTime<Utc>,
}

// 聊天服务
#[derive(Clone)]
pub struct ChatService {
    pub pool: PgPool,
    // 在线用户：user_id -> OnlineUser
    pub online_users: Arc<RwLock<HashMap<i64, OnlineUser>>>,
    // 用户的会话列表：user_id -> Set<conversation_id>
    pub user_conversations: Arc<RwLock<HashMap<i64, HashSet<i64>>>>,
}

impl ChatService {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            online_users: Arc::new(RwLock::new(HashMap::new())),
            user_conversations: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    // ==================== 用户在线状态管理 ====================

    // 用户上线
    pub async fn user_online(&self, user_id: i64, username: String) -> Result<Vec<Message>, DomainError> {
        // 1. 标记用户在线
        let mut online_users = self.online_users.write().await;
        online_users.insert(user_id, OnlineUser {
            user_id,
            username,
            connected_at: Utc::now(),
        });
        drop(online_users);

        // 2. 获取离线消息
        let offline_messages = self.get_offline_messages(user_id).await?;

        // 3. 删除离线消息记录
        self.delete_offline_messages(user_id).await?;

        Ok(offline_messages)
    }

    // 用户下线
    pub async fn user_offline(&self, user_id: i64) -> Result<(), DomainError> {
        let mut online_users = self.online_users.write().await;
        online_users.remove(&user_id);
        Ok(())
    }

    // 检查用户是否在线
    pub async fn is_user_online(&self, user_id: i64) -> bool {
        let online_users = self.online_users.read().await;
        online_users.contains_key(&user_id)
    }

    // 获取在线用户列表
    pub async fn get_online_users(&self) -> Vec<i64> {
        let online_users = self.online_users.read().await;
        online_users.keys().copied().collect()
    }

    // ==================== 会话管理 ====================

    // 创建一对一私聊会话
    pub async fn create_private_conversation(&self, user1_id: i64, user2_id: i64) -> Result<Conversation, DomainError> {
        // 检查是否已存在会话
        let existing = sqlx::query_as::<Postgres, Conversation>(
            r#"SELECT c.* FROM conversations c
               INNER JOIN conversation_members cm1 ON c.id = cm1.conversation_id AND cm1.user_id = $1
               INNER JOIN conversation_members cm2 ON c.id = cm2.conversation_id AND cm2.user_id = $2
               WHERE c.conversation_type = 'private' AND c.deleted_at IS NULL
               AND cm1.left_at IS NULL AND cm2.left_at IS NULL
               LIMIT 1"#
        )
        .bind(user1_id)
        .bind(user2_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        if let Some(conv) = existing {
            return Ok(conv);
        }

        // 创建新会话
        let mut tx = self.pool.begin().await.map_err(|e| DomainError::Db(e.to_string()))?;

        let conversation = sqlx::query_as::<Postgres, Conversation>(
            r#"INSERT INTO conversations (conversation_type) VALUES ('private') RETURNING *"#
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        // 添加成员
        sqlx::query(
            r#"INSERT INTO conversation_members (conversation_id, user_id) VALUES ($1, $2), ($1, $3)"#
        )
        .bind(conversation.id)
        .bind(user1_id)
        .bind(user2_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        tx.commit().await.map_err(|e| DomainError::Db(e.to_string()))?;

        Ok(conversation)
    }

    // 创建群聊（带边界检查）
    pub async fn create_group_conversation(&self, owner_id: i64, name: String, member_ids: Vec<i64>) -> Result<Conversation, DomainError> {
        // 1. 参数验证
        if name.trim().is_empty() {
            return Err(DomainError::Validation("群聊名称不能为空".into()));
        }

        if name.len() > 100 {
            return Err(DomainError::Validation("群聊名称过长（最大100字符）".into()));
        }

        // 检查成员数量（包括群主）
        let total_members = member_ids.len() + 1;
        if total_members > MAX_GROUP_MEMBERS {
            return Err(DomainError::Validation(
                format!("群成员数量超过限制（最大{}人）", MAX_GROUP_MEMBERS)
            ));
        }

        // 2. 开启事务（带超时）
        let mut tx = tokio::time::timeout(
            TRANSACTION_TIMEOUT,
            self.pool.begin()
        )
        .await
        .map_err(|_| DomainError::Timeout)?
        .map_err(|e| DomainError::Db(e.to_string()))?;

        // 3. 创建群聊
        let conversation = sqlx::query_as::<Postgres, Conversation>(
            r#"INSERT INTO conversations (conversation_type, name, owner_id) 
               VALUES ('group', $1, $2) RETURNING *"#
        )
        .bind(&name)
        .bind(owner_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        // 4. 添加群主
        sqlx::query(
            r#"INSERT INTO conversation_members (conversation_id, user_id) VALUES ($1, $2)"#
        )
        .bind(conversation.id)
        .bind(owner_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        // 5. 添加其他成员（去重）
        let unique_members: HashSet<i64> = member_ids.into_iter().filter(|&id| id != owner_id).collect();
        for member_id in unique_members {
            sqlx::query(
                r#"INSERT INTO conversation_members (conversation_id, user_id) VALUES ($1, $2)"#
            )
            .bind(conversation.id)
            .bind(member_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| DomainError::Db(e.to_string()))?;
        }

        // 6. 提交事务（带超时）
        tokio::time::timeout(
            Duration::from_secs(5),
            tx.commit()
        )
        .await
        .map_err(|_| DomainError::Timeout)?
        .map_err(|e| DomainError::Db(e.to_string()))?;

        Ok(conversation)
    }

    // 邀请用户加入群聊（带边界检查）
    pub async fn invite_to_group(&self, conversation_id: i64, inviter_id: i64, user_ids: Vec<i64>) -> Result<(), DomainError> {
        // 1. 参数验证
        if user_ids.is_empty() {
            return Err(DomainError::Validation("邀请用户列表不能为空".into()));
        }

        // 2. 开启事务（带超时）
        let mut tx = tokio::time::timeout(
            TRANSACTION_TIMEOUT,
            self.pool.begin()
        )
        .await
        .map_err(|_| DomainError::Timeout)?
        .map_err(|e| DomainError::Db(e.to_string()))?;

        // 3. 获取会话锁
        self.advisory_lock_tx(&mut tx, conversation_id).await?;

        // 4. 检查会话是否存在且未删除
        let conversation_status = sqlx::query_as::<Postgres, (ConversationType, Option<DateTime<Utc>>)>(
            r#"SELECT conversation_type, deleted_at FROM conversations WHERE id = $1 FOR UPDATE NOWAIT"#
        )
        .bind(conversation_id)
        .fetch_optional(&mut *tx)
        .await;

        let (conv_type, deleted_at) = match conversation_status {
            Ok(Some(status)) => status,
            Ok(None) => return Err(DomainError::NotFound),
            Err(e) => {
                let err_msg = e.to_string();
                if err_msg.contains("could not obtain lock") {
                    return Err(DomainError::Locked);
                }
                return Err(DomainError::Db(err_msg));
            }
        };

        if deleted_at.is_some() {
            return Err(DomainError::Gone);
        }

        if conv_type != ConversationType::Group {
            return Err(DomainError::Validation("只能邀请用户加入群聊".into()));
        }

        // 5. 检查邀请者是否是群成员
        let is_member: (i64,) = sqlx::query_as(
            r#"SELECT COUNT(*) FROM conversation_members 
               WHERE conversation_id = $1 AND user_id = $2 AND left_at IS NULL"#
        )
        .bind(conversation_id)
        .bind(inviter_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        if is_member.0 == 0 {
            return Err(DomainError::Validation("您不是该群成员".into()));
        }

        // 6. 检查当前群成员数量
        let current_count: (i64,) = sqlx::query_as(
            r#"SELECT COUNT(*) FROM conversation_members 
               WHERE conversation_id = $1 AND left_at IS NULL"#
        )
        .bind(conversation_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        let new_total = current_count.0 as usize + user_ids.len();
        if new_total > MAX_GROUP_MEMBERS {
            return Err(DomainError::Validation(
                format!("群成员数量将超过限制（最大{}人）", MAX_GROUP_MEMBERS)
            ));
        }

        // 7. 添加成员（去重）
        let unique_users: HashSet<i64> = user_ids.into_iter().collect();
        for user_id in unique_users {
            sqlx::query(
                r#"INSERT INTO conversation_members (conversation_id, user_id) 
                   VALUES ($1, $2) 
                   ON CONFLICT (conversation_id, user_id) WHERE left_at IS NULL DO NOTHING"#
            )
            .bind(conversation_id)
            .bind(user_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| DomainError::Db(e.to_string()))?;
        }

        // 8. 提交事务（带超时）
        tokio::time::timeout(
            Duration::from_secs(5),
            tx.commit()
        )
        .await
        .map_err(|_| DomainError::Timeout)?
        .map_err(|e| DomainError::Db(e.to_string()))?;

        Ok(())
    }

    // 搜索好友（用于邀请）
    pub async fn search_users_for_invite(&self, query: &str, limit: i64) -> Result<Vec<(i64, String)>, DomainError> {
        let users = sqlx::query_as::<Postgres, (i64, String)>(
            r#"SELECT id, username FROM users 
               WHERE username ILIKE $1 
               LIMIT $2"#
        )
        .bind(format!("%{}%", query))
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        Ok(users)
    }

    // ==================== 消息发送 ====================

    // 发送消息（带完整的边界检查）
    pub async fn send_message(
        &self,
        conversation_id: i64,
        sender_id: i64,
        message_type: MessageType,
        content: String,
        file_url: Option<String>,
        file_name: Option<String>,
        file_size: Option<i64>,
    ) -> Result<Message, DomainError> {
        // 1. 参数验证
        self.validate_message_params(&content, &file_size)?;

        // 2. 检查连续发送消息间隔
        self.check_message_interval(sender_id, conversation_id).await?;

        // 3. 开启事务（带超时）
        let mut tx = tokio::time::timeout(
            TRANSACTION_TIMEOUT,
            self.pool.begin()
        )
        .await
        .map_err(|_| DomainError::Timeout)?
        .map_err(|e| DomainError::Db(e.to_string()))?;

        // 4. 获取会话锁（带超时）
        self.advisory_lock_tx(&mut tx, conversation_id).await?;

        // 5. 检查会话是否存在且未删除
        let conversation_status = sqlx::query_as::<Postgres, (Option<DateTime<Utc>>,)>(
            r#"SELECT deleted_at FROM conversations WHERE id = $1 FOR UPDATE NOWAIT"#
        )
        .bind(conversation_id)
        .fetch_optional(&mut *tx)
        .await;

        match conversation_status {
            Ok(Some((deleted_at,))) => {
                if deleted_at.is_some() {
                    return Err(DomainError::Gone); // 会话已删除
                }
            }
            Ok(None) => return Err(DomainError::NotFound), // 会话不存在
            Err(e) => {
                let err_msg = e.to_string();
                if err_msg.contains("could not obtain lock") {
                    return Err(DomainError::Locked); // 会话正在被操作
                }
                return Err(DomainError::Db(err_msg));
            }
        }

        // 6. 检查发送者是否是会话成员（使用 NOWAIT）
        let is_member = sqlx::query_as::<Postgres, (i64,)>(
            r#"SELECT COUNT(*) FROM conversation_members 
               WHERE conversation_id = $1 AND user_id = $2 AND left_at IS NULL
               FOR UPDATE NOWAIT"#
        )
        .bind(conversation_id)
        .bind(sender_id)
        .fetch_one(&mut *tx)
        .await;

        match is_member {
            Ok((count,)) => {
                if count == 0 {
                    return Err(DomainError::Validation("您不是该会话成员".into()));
                }
            }
            Err(e) => {
                let err_msg = e.to_string();
                if err_msg.contains("could not obtain lock") {
                    return Err(DomainError::Locked); // 成员关系正在被修改
                }
                return Err(DomainError::Db(err_msg));
            }
        }

        // 7. 保存消息
        let message = sqlx::query_as::<Postgres, Message>(
            r#"INSERT INTO messages (conversation_id, sender_id, message_type, content, file_url, file_name, file_size)
               VALUES ($1, $2, $3, $4, $5, $6, $7)
               RETURNING *"#
        )
        .bind(conversation_id)
        .bind(sender_id)
        .bind(&message_type)
        .bind(&content)
        .bind(&file_url)
        .bind(&file_name)
        .bind(file_size)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        // 8. 获取会话所有成员
        let members: Vec<(i64,)> = sqlx::query_as(
            r#"SELECT user_id FROM conversation_members 
               WHERE conversation_id = $1 AND left_at IS NULL"#
        )
        .bind(conversation_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        // 9. 检查哪些用户离线，保存离线消息
        for (member_id,) in members {
            if member_id != sender_id && !self.is_user_online(member_id).await {
                sqlx::query(
                    r#"INSERT INTO offline_messages (user_id, message_id) VALUES ($1, $2)"#
                )
                .bind(member_id)
                .bind(message.id)
                .execute(&mut *tx)
                .await
                .map_err(|e| DomainError::Db(e.to_string()))?;
            }
        }

        // 10. 提交事务（带超时）
        tokio::time::timeout(
            Duration::from_secs(5),
            tx.commit()
        )
        .await
        .map_err(|_| DomainError::Timeout)?
        .map_err(|e| DomainError::Db(e.to_string()))?;

        Ok(message)
    }

    // 参数验证
    fn validate_message_params(&self, content: &str, file_size: &Option<i64>) -> Result<(), DomainError> {
        // 检查消息长度
        if content.len() > MAX_MESSAGE_LENGTH {
            return Err(DomainError::Validation(
                format!("消息长度超过限制（最大{}字符）", MAX_MESSAGE_LENGTH)
            ));
        }

        // 检查文件大小
        if let Some(size) = file_size {
            if *size > MAX_FILE_SIZE {
                return Err(DomainError::Validation(
                    format!("文件大小超过限制（最大{}MB）", MAX_FILE_SIZE / 1024 / 1024)
                ));
            }
        }

        Ok(())
    }

    // 检查连续发送消息间隔
    async fn check_message_interval(&self, sender_id: i64, conversation_id: i64) -> Result<(), DomainError> {
        let last_message = sqlx::query_as::<Postgres, (DateTime<Utc>,)>(
            r#"SELECT created_at FROM messages 
               WHERE sender_id = $1 AND conversation_id = $2 
               ORDER BY created_at DESC 
               LIMIT 1"#
        )
        .bind(sender_id)
        .bind(conversation_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        if let Some((last_time,)) = last_message {
            let now = Utc::now();
            let elapsed = (now - last_time).num_seconds();
            
            if elapsed < MESSAGE_INTERVAL_SECONDS {
                return Err(DomainError::TooManyRequests);
            }
        }

        Ok(())
    }

    // 顾问锁（带超时）
    async fn advisory_lock_tx(&self, tx: &mut Transaction<'_, Postgres>, conversation_id: i64) -> Result<(), DomainError> {
        // 设置锁超时
        sqlx::query::<Postgres>("SET LOCAL lock_timeout = $1")
            .bind(format!("{}s", LOCK_TIMEOUT_SECONDS))
            .execute(&mut **tx)
            .await
            .map_err(|e| DomainError::Db(format!("设置锁超时失败: {}", e)))?;

        // 获取顾问锁
        let result = sqlx::query::<Postgres>("SELECT pg_advisory_xact_lock($1)")
            .bind(conversation_id)
            .execute(&mut **tx)
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) => {
                let err_msg = e.to_string();
                if err_msg.contains("timeout") || err_msg.contains("canceling statement due to lock timeout") {
                    Err(DomainError::Locked)
                } else {
                    Err(DomainError::Db(format!("获取锁失败: {}", err_msg)))
                }
            }
        }
    }

    // ==================== 离线消息管理 ====================

    // 保存离线消息
    async fn save_offline_message(&self, user_id: i64, message_id: i64) -> Result<(), DomainError> {
        sqlx::query(
            r#"INSERT INTO offline_messages (user_id, message_id) VALUES ($1, $2)"#
        )
        .bind(user_id)
        .bind(message_id)
        .execute(&self.pool)
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        Ok(())
    }

    // 获取离线消息
    async fn get_offline_messages(&self, user_id: i64) -> Result<Vec<Message>, DomainError> {
        let messages = sqlx::query_as::<Postgres, Message>(
            r#"SELECT m.* FROM messages m
               INNER JOIN offline_messages om ON m.id = om.message_id
               WHERE om.user_id = $1
               ORDER BY m.created_at ASC"#
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        Ok(messages)
    }

    // 删除离线消息
    async fn delete_offline_messages(&self, user_id: i64) -> Result<(), DomainError> {
        sqlx::query(
            r#"DELETE FROM offline_messages WHERE user_id = $1"#
        )
        .bind(user_id)
        .execute(&self.pool)
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        Ok(())
    }

    // ==================== 辅助方法 ====================

    // 获取会话信息
    async fn get_conversation(&self, conversation_id: i64) -> Result<Conversation, DomainError> {
        let conversation = sqlx::query_as::<Postgres, Conversation>(
            r#"SELECT * FROM conversations WHERE id = $1 AND deleted_at IS NULL"#
        )
        .bind(conversation_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        conversation.ok_or(DomainError::NotFound)
    }

    // 检查用户是否是会话成员
    async fn is_conversation_member(&self, conversation_id: i64, user_id: i64) -> Result<bool, DomainError> {
        let count: (i64,) = sqlx::query_as(
            r#"SELECT COUNT(*) FROM conversation_members 
               WHERE conversation_id = $1 AND user_id = $2 AND left_at IS NULL"#
        )
        .bind(conversation_id)
        .bind(user_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        Ok(count.0 > 0)
    }

    // 获取会话所有成员ID
    async fn get_conversation_members(&self, conversation_id: i64) -> Result<Vec<i64>, DomainError> {
        let members: Vec<(i64,)> = sqlx::query_as(
            r#"SELECT user_id FROM conversation_members 
               WHERE conversation_id = $1 AND left_at IS NULL"#
        )
        .bind(conversation_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        Ok(members.into_iter().map(|(id,)| id).collect())
    }

    // 获取用户的会话列表
    pub async fn get_user_conversations(&self, user_id: i64) -> Result<Vec<Conversation>, DomainError> {
        let conversations = sqlx::query_as::<Postgres, Conversation>(
            r#"SELECT c.* FROM conversations c
               INNER JOIN conversation_members cm ON c.id = cm.conversation_id
               WHERE cm.user_id = $1 AND cm.left_at IS NULL AND c.deleted_at IS NULL
               ORDER BY c.id DESC"#
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        Ok(conversations)
    }

    // 获取会话消息历史（带权限检查）
    pub async fn get_conversation_messages(&self, conversation_id: i64, user_id: i64, limit: i64, offset: i64) -> Result<Vec<Message>, DomainError> {
        // 1. 检查会话是否存在
        let conversation = self.get_conversation(conversation_id).await?;

        // 2. 检查用户是否是会话成员
        let is_member = self.is_conversation_member(conversation_id, user_id).await?;
        if !is_member {
            return Err(DomainError::Validation("您不是该会话成员".into()));
        }

        // 3. 获取消息历史
        let messages = sqlx::query_as::<Postgres, Message>(
            r#"SELECT * FROM messages 
               WHERE conversation_id = $1 AND deleted_at IS NULL
               ORDER BY created_at DESC
               LIMIT $2 OFFSET $3"#
        )
        .bind(conversation_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        Ok(messages)
    }

    // 退出群聊（带边界检查）
    pub async fn leave_group(&self, conversation_id: i64, user_id: i64) -> Result<(), DomainError> {
        // 1. 开启事务（带超时）
        let mut tx = tokio::time::timeout(
            TRANSACTION_TIMEOUT,
            self.pool.begin()
        )
        .await
        .map_err(|_| DomainError::Timeout)?
        .map_err(|e| DomainError::Db(e.to_string()))?;

        // 2. 获取会话锁
        self.advisory_lock_tx(&mut tx, conversation_id).await?;

        // 3. 检查会话状态
        let conversation_info = sqlx::query_as::<Postgres, (ConversationType, Option<i64>, Option<DateTime<Utc>>)>(
            r#"SELECT conversation_type, owner_id, deleted_at FROM conversations WHERE id = $1 FOR UPDATE NOWAIT"#
        )
        .bind(conversation_id)
        .fetch_optional(&mut *tx)
        .await;

        let (conv_type, owner_id, deleted_at) = match conversation_info {
            Ok(Some(info)) => info,
            Ok(None) => return Err(DomainError::NotFound),
            Err(e) => {
                let err_msg = e.to_string();
                if err_msg.contains("could not obtain lock") {
                    return Err(DomainError::Locked);
                }
                return Err(DomainError::Db(err_msg));
            }
        };

        if deleted_at.is_some() {
            return Err(DomainError::Gone);
        }

        if conv_type != ConversationType::Group {
            return Err(DomainError::Validation("只能退出群聊".into()));
        }

        // 4. 检查是否是群主
        if Some(user_id) == owner_id {
            return Err(DomainError::Validation("群主不能退出群聊，请先转让群主或解散群聊".into()));
        }

        // 5. 检查用户是否是成员
        let member_status = sqlx::query_as::<Postgres, (Option<DateTime<Utc>>,)>(
            r#"SELECT left_at FROM conversation_members 
               WHERE conversation_id = $1 AND user_id = $2 
               FOR UPDATE NOWAIT"#
        )
        .bind(conversation_id)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await;

        match member_status {
            Ok(Some((left_at,))) => {
                if left_at.is_some() {
                    return Err(DomainError::Gone); // 已经退出
                }
            }
            Ok(None) => return Err(DomainError::NotFound), // 不是成员
            Err(e) => {
                let err_msg = e.to_string();
                if err_msg.contains("could not obtain lock") {
                    return Err(DomainError::Locked);
                }
                return Err(DomainError::Db(err_msg));
            }
        }

        // 6. 标记用户退出
        sqlx::query(
            r#"UPDATE conversation_members SET left_at = NOW() 
               WHERE conversation_id = $1 AND user_id = $2 AND left_at IS NULL"#
        )
        .bind(conversation_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        // 7. 提交事务（带超时）
        tokio::time::timeout(
            Duration::from_secs(5),
            tx.commit()
        )
        .await
        .map_err(|_| DomainError::Timeout)?
        .map_err(|e| DomainError::Db(e.to_string()))?;

        Ok(())
    }

    // 删除会话（软删除，带边界检查）
    pub async fn delete_conversation(&self, conversation_id: i64, user_id: i64) -> Result<(), DomainError> {
        // 1. 开启事务（带超时）
        let mut tx = tokio::time::timeout(
            TRANSACTION_TIMEOUT,
            self.pool.begin()
        )
        .await
        .map_err(|_| DomainError::Timeout)?
        .map_err(|e| DomainError::Db(e.to_string()))?;

        // 2. 获取会话锁
        self.advisory_lock_tx(&mut tx, conversation_id).await?;

        // 3. 检查会话状态和权限
        let conversation_info = sqlx::query_as::<Postgres, (ConversationType, Option<i64>, Option<DateTime<Utc>>)>(
            r#"SELECT conversation_type, owner_id, deleted_at FROM conversations WHERE id = $1 FOR UPDATE NOWAIT"#
        )
        .bind(conversation_id)
        .fetch_optional(&mut *tx)
        .await;

        let (conv_type, owner_id, deleted_at) = match conversation_info {
            Ok(Some(info)) => info,
            Ok(None) => return Err(DomainError::NotFound),
            Err(e) => {
                let err_msg = e.to_string();
                if err_msg.contains("could not obtain lock") {
                    return Err(DomainError::Locked);
                }
                return Err(DomainError::Db(err_msg));
            }
        };

        if deleted_at.is_some() {
            return Err(DomainError::Gone); // 已经删除
        }

        // 4. 权限检查
        if conv_type == ConversationType::Group {
            // 群聊只有群主可以删除
            if Some(user_id) != owner_id {
                return Err(DomainError::Validation("只有群主可以解散群聊".into()));
            }
        } else {
            // 私聊需要是成员之一
            let is_member = sqlx::query_as::<Postgres, (i64,)>(
                r#"SELECT COUNT(*) FROM conversation_members 
                   WHERE conversation_id = $1 AND user_id = $2 AND left_at IS NULL"#
            )
            .bind(conversation_id)
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| DomainError::Db(e.to_string()))?;

            if is_member.0 == 0 {
                return Err(DomainError::Validation("您不是该会话成员".into()));
            }
        }

        // 5. 软删除会话
        let rows_affected = sqlx::query(
            r#"UPDATE conversations SET deleted_at = NOW() WHERE id = $1 AND deleted_at IS NULL"#
        )
        .bind(conversation_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?
        .rows_affected();

        if rows_affected == 0 {
            return Err(DomainError::Gone); // 并发情况下已被删除
        }

        // 6. 软删除所有消息
        sqlx::query(
            r#"UPDATE messages SET deleted_at = NOW() WHERE conversation_id = $1 AND deleted_at IS NULL"#
        )
        .bind(conversation_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        // 7. 删除离线消息
        sqlx::query(
            r#"DELETE FROM offline_messages WHERE message_id IN 
               (SELECT id FROM messages WHERE conversation_id = $1)"#
        )
        .bind(conversation_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        // 8. 提交事务（带超时）
        tokio::time::timeout(
            Duration::from_secs(5),
            tx.commit()
        )
        .await
        .map_err(|_| DomainError::Timeout)?
        .map_err(|e| DomainError::Db(e.to_string()))?;

        Ok(())
    }
}
