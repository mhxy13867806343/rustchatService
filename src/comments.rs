use crate::errors::DomainError;
use crate::rate_limit::RateLimiter;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};
// use uuid::Uuid; // currently unused

#[derive(Debug, Clone)]
pub struct CommentService {
    pub pool: PgPool,
    pub limiter: RateLimiter,
}

#[derive(Debug, Clone)]
pub struct CreateCommentInput {
    pub post_id: i64,
    pub author_id: i64,
    pub parent_comment_id: Option<i64>,
    pub content: String,
    pub at_user_id: Option<i64>,
    pub idempotency_key: String,
    pub ip_key: String,
}

#[derive(sqlx::FromRow, Debug, Clone)]
pub struct CommentRow {
    pub id: i64,
    pub post_id: i64,
    pub author_id: i64,
    pub parent_comment_id: Option<i64>,
    pub content: String,
    pub at_user_id: Option<i64>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

impl CommentService {
    pub fn new(pool: PgPool, limiter: RateLimiter) -> Self {
        Self { pool, limiter }
    }

    // 顾问锁：以post_id作为全局锁键
    async fn advisory_lock_tx(&self, tx: &mut Transaction<'_, Postgres>, post_id: i64) -> Result<(), DomainError> {
        sqlx::query::<Postgres>("SELECT pg_advisory_xact_lock($1)")
            .bind(post_id)
            .execute(tx.as_mut())
            .await
            .map_err(|e| DomainError::Db(e.to_string()))?;
        Ok(())
    }

    // 行级锁获取Post以判断锁帖/删除
    async fn load_post_for_update(&self, tx: &mut Transaction<'_, Postgres>, post_id: i64) -> Result<(Option<DateTime<Utc>>, Option<DateTime<Utc>>), DomainError> {
        let row = sqlx::query_as::<Postgres, (Option<DateTime<Utc>>, Option<DateTime<Utc>>)>(
            r#"SELECT locked_at, deleted_at FROM posts WHERE id = $1 FOR UPDATE"#
        )
        .bind(post_id)
        .fetch_optional(tx.as_mut())
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        match row {
            None => Err(DomainError::NotFound),
            Some((locked_at, deleted_at)) => Ok((locked_at, deleted_at)),
        }
    }

    pub async fn create_comment(&self, input: CreateCommentInput) -> Result<CommentRow, DomainError> {
        // 速率限制：用户维度与IP维度
        let ok_user = self.limiter.check_and_consume(&format!("u:{}:comment", input.author_id), 10, 5).await.map_err(|e| DomainError::Db(e.to_string()))?;
        let ok_ip = self.limiter.check_and_consume(&format!("ip:{}:comment", input.ip_key), 20, 10).await.map_err(|e| DomainError::Db(e.to_string()))?;
        if !(ok_user && ok_ip) { return Err(DomainError::TooManyRequests); }

        let mut tx = self.pool.begin().await.map_err(|e| DomainError::Db(e.to_string()))?;
        self.advisory_lock_tx(&mut tx, input.post_id).await?;

        let (locked_at, deleted_at) = self.load_post_for_update(&mut tx, input.post_id).await?;
        if deleted_at.is_some() { return Err(DomainError::Gone); }
        if locked_at.is_some() { return Err(DomainError::Locked); }

        // 最大楼层深度限制（两层）
        if let Some(parent_id) = input.parent_comment_id {
            // 检查父评论是否是一级，防止超过两层
            let parent = sqlx::query_as::<Postgres, (Option<i64>, Option<DateTime<Utc>>)>(
                r#"SELECT parent_comment_id, deleted_at FROM comments WHERE id = $1 FOR UPDATE"#
            )
            .bind(parent_id)
            .fetch_optional(tx.as_mut())
            .await
            .map_err(|e| DomainError::Db(e.to_string()))?;
            match parent {
                Some((parent_comment_id, deleted_at)) => {
                    if deleted_at.is_some() { return Err(DomainError::Gone); }
                    if parent_comment_id.is_some() { return Err(DomainError::Validation("超过最大楼层深度".into())); }
                }
                None => return Err(DomainError::NotFound),
            }
        }

        // 幂等插入评论
        let inserted = sqlx::query_as::<Postgres, CommentRow>(
            r#"INSERT INTO comments (post_id, author_id, parent_comment_id, content, at_user_id, idempotency_key)
               SELECT $1, $2, $3, $4, $5, $6
               WHERE EXISTS (SELECT 1 FROM posts WHERE id = $1 AND deleted_at IS NULL)
               ON CONFLICT (author_id, post_id, idempotency_key)
               DO UPDATE SET updated_at = NOW()
               RETURNING id, post_id, author_id, parent_comment_id, content, at_user_id, deleted_at, created_at"#
        )
        .bind(input.post_id)
        .bind(input.author_id)
        .bind(input.parent_comment_id)
        .bind(input.content)
        .bind(input.at_user_id)
        .bind(input.idempotency_key)
        .fetch_one(tx.as_mut())
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        // 发布事件（简化：仅发送ID）
        sqlx::query::<Postgres>("SELECT pg_notify('events', $1)")
            .bind(format!(r#"{{"type":"comment_created","id":{}}}"#, inserted.id))
            .execute(tx.as_mut())
            .await
            .map_err(|e| DomainError::Db(e.to_string()))?;

        tx.commit().await.map_err(|e| DomainError::Db(e.to_string()))?;
        Ok(inserted)
    }

    pub async fn delete_post_soft(&self, post_id: i64, _actor_id: i64) -> Result<(), DomainError> {
        let mut tx = self.pool.begin().await.map_err(|e| DomainError::Db(e.to_string()))?;
        self.advisory_lock_tx(&mut tx, post_id).await?;

        // 软删帖子
        sqlx::query::<Postgres>("UPDATE posts SET deleted_at = NOW() WHERE id = $1 AND deleted_at IS NULL")
            .bind(post_id)
            .execute(tx.as_mut())
            .await
            .map_err(|e| DomainError::Db(e.to_string()))?;

        // 软删帖子下的所有评论
        sqlx::query::<Postgres>("UPDATE comments SET deleted_at = NOW() WHERE post_id = $1 AND deleted_at IS NULL")
            .bind(post_id)
            .execute(tx.as_mut())
            .await
            .map_err(|e| DomainError::Db(e.to_string()))?;

        // 软删帖子上的所有反应
        sqlx::query::<Postgres>("UPDATE reactions SET deleted_at = NOW() WHERE resource_type = 1 AND resource_id = $1 AND deleted_at IS NULL")
            .bind(post_id)
            .execute(tx.as_mut())
            .await
            .map_err(|e| DomainError::Db(e.to_string()))?;

        // 软删该帖子评论上的所有反应
        sqlx::query::<Postgres>("UPDATE reactions SET deleted_at = NOW() WHERE resource_type = 2 AND resource_id IN (SELECT id FROM comments WHERE post_id = $1) AND deleted_at IS NULL")
            .bind(post_id)
            .execute(tx.as_mut())
            .await
            .map_err(|e| DomainError::Db(e.to_string()))?;

        sqlx::query::<Postgres>("SELECT pg_notify('events', $1)")
            .bind(format!(r#"{{"type":"post_deleted","id":{}}}"#, post_id))
            .execute(tx.as_mut())
            .await
            .map_err(|e| DomainError::Db(e.to_string()))?;

        tx.commit().await.map_err(|e| DomainError::Db(e.to_string()))?;
        Ok(())
    }

    pub async fn react_idempotent(&self, resource_type: i16, resource_id: i64, reactor_id: i64, reaction_type: i16, idempotency_key: String) -> Result<(), DomainError> {
        let mut tx = self.pool.begin().await.map_err(|e| DomainError::Db(e.to_string()))?;
        // 针对帖子使用顾问锁，评论可按分桶hash锁
        let lock_key = if resource_type == 1 { resource_id } else { (resource_id % 1024) as i64 };
        self.advisory_lock_tx(&mut tx, lock_key).await?;

        sqlx::query::<Postgres>(
            r#"INSERT INTO reactions(resource_type, resource_id, reactor_id, reaction_type, idempotency_key)
               VALUES ($1, $2, $3, $4, $5)
               ON CONFLICT (reactor_id, resource_type, resource_id, reaction_type, idempotency_key)
               DO UPDATE SET updated_at = NOW()"#
        )
        .bind(resource_type)
        .bind(resource_id)
        .bind(reactor_id)
        .bind(reaction_type)
        .bind(idempotency_key)
        .execute(tx.as_mut())
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        sqlx::query::<Postgres>("SELECT pg_notify('events', $1)")
            .bind(format!(r#"{{"type":"reaction","rid":{},"rt":{}}}"#, resource_id, reaction_type))
        .execute(tx.as_mut())
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        tx.commit().await.map_err(|e| DomainError::Db(e.to_string()))?;
        Ok(())
    }

    // 批量评论：一次插入多条（满足存在post且未删）
    pub async fn batch_create_comments(&self, inputs: Vec<CreateCommentInput>) -> Result<Vec<CommentRow>, DomainError> {
        if inputs.is_empty() { return Ok(vec![]); }
        // 速率限制：对每个作者简单校验（可优化为分组一次校验）
        for i in &inputs {
            let ok_user = self.limiter.check_and_consume(&format!("u:{}:comment", i.author_id), 20, 10).await.map_err(|e| DomainError::Db(e.to_string()))?;
            if !ok_user { return Err(DomainError::TooManyRequests); }
        }

        let mut tx = self.pool.begin().await.map_err(|e| DomainError::Db(e.to_string()))?;
        // 顾问锁：以首个post_id为例（生产可按分桶）
        self.advisory_lock_tx(&mut tx, inputs[0].post_id).await?;

        let mut rows = Vec::with_capacity(inputs.len());
        for i in inputs {
            let row = sqlx::query_as::<Postgres, CommentRow>(
                r#"INSERT INTO comments (post_id, author_id, parent_comment_id, content, at_user_id, idempotency_key)
                   SELECT $1, $2, $3, $4, $5, $6
                   WHERE EXISTS (SELECT 1 FROM posts WHERE id = $1 AND deleted_at IS NULL)
                   ON CONFLICT (author_id, post_id, idempotency_key)
                   DO UPDATE SET updated_at = NOW()
                   RETURNING id, post_id, author_id, parent_comment_id, content, at_user_id, deleted_at, created_at"#
            )
            .bind(i.post_id)
            .bind(i.author_id)
            .bind(i.parent_comment_id)
            .bind(i.content)
            .bind(i.at_user_id)
            .bind(i.idempotency_key)
            .fetch_one(tx.as_mut())
            .await
            .map_err(|e| DomainError::Db(e.to_string()))?;
            rows.push(row);
        }

        // 事件通知（仅发送post_id）
        sqlx::query::<Postgres>("SELECT pg_notify('events', $1)")
            .bind(format!(r#"{{"type":"batch_comment","post_id":{}}}"#, rows.first().map(|r| r.post_id).unwrap_or_default()))
            .execute(tx.as_mut())
            .await
            .map_err(|e| DomainError::Db(e.to_string()))?;

        tx.commit().await.map_err(|e| DomainError::Db(e.to_string()))?;
        Ok(rows)
    }
}