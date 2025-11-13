use crate::errors::DomainError;
use crate::rate_limit::RateLimiter;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use std::time::Duration;
// use uuid::Uuid; // currently unused

#[derive(Debug, Clone)]
pub struct CommentService {
    pub pool: PgPool,
    pub limiter: RateLimiter,
}

// 配置常量
const COMMENT_INTERVAL_SECONDS: i64 = 3; // 连续评论最小间隔（秒）
const LOCK_TIMEOUT_SECONDS: i64 = 10; // 锁超时时间（秒）
const TRANSACTION_TIMEOUT: Duration = Duration::from_secs(30); // 事务超时时间

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

#[derive(Debug, Clone)]
pub enum PostStatus {
    Active,  // 正常
    Locked,  // 已锁定
}

impl CommentService {
    pub fn new(pool: PgPool, limiter: RateLimiter) -> Self {
        Self { pool, limiter }
    }

    // 顾问锁：以post_id作为全局锁键，带超时
    async fn advisory_lock_tx(&self, tx: &mut Transaction<'_, Postgres>, post_id: i64) -> Result<(), DomainError> {
        // 设置锁超时
        sqlx::query::<Postgres>("SET LOCAL lock_timeout = $1")
            .bind(format!("{}s", LOCK_TIMEOUT_SECONDS))
            .execute(tx.as_mut())
            .await
            .map_err(|e| DomainError::Db(format!("设置锁超时失败: {}", e)))?;

        // 获取顾问锁
        let result = sqlx::query::<Postgres>("SELECT pg_advisory_xact_lock($1)")
            .bind(post_id)
            .execute(tx.as_mut())
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) => {
                let err_msg = e.to_string();
                if err_msg.contains("timeout") || err_msg.contains("canceling statement due to lock timeout") {
                    Err(DomainError::Locked) // 锁超时，资源被锁定
                } else {
                    Err(DomainError::Db(format!("获取锁失败: {}", err_msg)))
                }
            }
        }
    }

    // 检查用户是否在短时间内连续评论
    async fn check_comment_interval(&self, author_id: i64, post_id: i64) -> Result<(), DomainError> {
        // 查询用户在该帖子下的最后一条评论时间
        let last_comment = sqlx::query_as::<Postgres, (DateTime<Utc>,)>(
            r#"SELECT created_at FROM comments 
               WHERE author_id = $1 AND post_id = $2 
               ORDER BY created_at DESC 
               LIMIT 1"#
        )
        .bind(author_id)
        .bind(post_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        if let Some((last_time,)) = last_comment {
            let now = Utc::now();
            let elapsed = (now - last_time).num_seconds();
            
            if elapsed < COMMENT_INTERVAL_SECONDS {
                let wait_seconds = COMMENT_INTERVAL_SECONDS - elapsed;
                return Err(DomainError::TooManyRequests);
            }
        }

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
        // 1. 速率限制：用户维度与IP维度
        let ok_user = self.limiter.check_and_consume(&format!("u:{}:comment", input.author_id), 10, 5).await.map_err(|e| DomainError::Db(e.to_string()))?;
        let ok_ip = self.limiter.check_and_consume(&format!("ip:{}:comment", input.ip_key), 20, 10).await.map_err(|e| DomainError::Db(e.to_string()))?;
        if !(ok_user && ok_ip) { 
            return Err(DomainError::TooManyRequests); 
        }

        // 2. 检查连续评论间隔（防止短时间内重复评论）
        self.check_comment_interval(input.author_id, input.post_id).await?;

        // 3. 开启事务（带超时）
        let mut tx = tokio::time::timeout(
            TRANSACTION_TIMEOUT,
            self.pool.begin()
        )
        .await
        .map_err(|_| DomainError::Db("事务开启超时".into()))?
        .map_err(|e| DomainError::Db(e.to_string()))?;

        // 4. 获取顾问锁（带超时）
        self.advisory_lock_tx(&mut tx, input.post_id).await?;

        // 5. 检查帖子状态（是否存在、是否删除、是否锁定）
        let post_status = sqlx::query_as::<Postgres, (Option<DateTime<Utc>>, Option<DateTime<Utc>>)>(
            r#"SELECT locked_at, deleted_at FROM posts WHERE id = $1 FOR UPDATE NOWAIT"#
        )
        .bind(input.post_id)
        .fetch_optional(tx.as_mut())
        .await;

        let (locked_at, deleted_at) = match post_status {
            Ok(Some(status)) => status,
            Ok(None) => return Err(DomainError::NotFound), // 帖子不存在
            Err(e) => {
                let err_msg = e.to_string();
                if err_msg.contains("could not obtain lock") {
                    return Err(DomainError::Locked); // 帖子正在被操作
                }
                return Err(DomainError::Db(err_msg));
            }
        };

        if deleted_at.is_some() { 
            return Err(DomainError::Gone); // 帖子已删除，不能评论
        }
        if locked_at.is_some() { 
            return Err(DomainError::Locked); // 帖子已锁定，不能评论
        }

        // 6. 如果是二级回复，检查父评论状态
        if let Some(parent_id) = input.parent_comment_id {
            let parent_status = sqlx::query_as::<Postgres, (Option<i64>, Option<DateTime<Utc>>)>(
                r#"SELECT parent_comment_id, deleted_at FROM comments WHERE id = $1 FOR UPDATE NOWAIT"#
            )
            .bind(parent_id)
            .fetch_optional(tx.as_mut())
            .await;
            
            match parent_status {
                Ok(Some((parent_comment_id, deleted_at))) => {
                    // 检查父评论是否已删除
                    if deleted_at.is_some() { 
                        return Err(DomainError::Gone); // 父评论已删除，不能回复
                    }
                    // 检查是否超过最大楼层深度（只允许二层）
                    if parent_comment_id.is_some() { 
                        return Err(DomainError::Validation("超过最大楼层深度，只支持二层评论".into())); 
                    }
                }
                Ok(None) => return Err(DomainError::NotFound), // 父评论不存在
                Err(e) => {
                    let err_msg = e.to_string();
                    if err_msg.contains("could not obtain lock") {
                        return Err(DomainError::Locked); // 父评论正在被操作（可能正在删除）
                    }
                    return Err(DomainError::Db(err_msg));
                }
            }
        }

        // 7. 幂等插入评论（只有在帖子未删除时才能插入）
        let insert_result = sqlx::query_as::<Postgres, CommentRow>(
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
        .fetch_optional(tx.as_mut())
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        let inserted = match insert_result {
            Some(row) => row,
            None => {
                // 插入失败，说明帖子在事务期间被删除了
                return Err(DomainError::Gone);
            }
        };

        // 8. 发布事件通知
        sqlx::query::<Postgres>("SELECT pg_notify('events', $1)")
            .bind(format!(r#"{{"type":"comment_created","id":{}}}"#, inserted.id))
            .execute(tx.as_mut())
            .await
            .map_err(|e| DomainError::Db(e.to_string()))?;

        // 9. 提交事务（带超时）
        tokio::time::timeout(
            Duration::from_secs(5),
            tx.commit()
        )
        .await
        .map_err(|_| DomainError::Db("事务提交超时".into()))?
        .map_err(|e| DomainError::Db(e.to_string()))?;

        Ok(inserted)
    }

    pub async fn delete_post_soft(&self, post_id: i64, _actor_id: i64) -> Result<(), DomainError> {
        // 1. 开启事务（带超时）
        let mut tx = tokio::time::timeout(
            TRANSACTION_TIMEOUT,
            self.pool.begin()
        )
        .await
        .map_err(|_| DomainError::Db("事务开启超时".into()))?
        .map_err(|e| DomainError::Db(e.to_string()))?;

        // 2. 获取顾问锁（带超时）
        self.advisory_lock_tx(&mut tx, post_id).await?;

        // 3. 检查帖子是否存在及状态（使用 NOWAIT 避免阻塞）
        let post_status = sqlx::query_as::<Postgres, (Option<DateTime<Utc>>,)>(
            r#"SELECT deleted_at FROM posts WHERE id = $1 FOR UPDATE NOWAIT"#
        )
        .bind(post_id)
        .fetch_optional(tx.as_mut())
        .await;

        match post_status {
            Ok(Some((deleted_at,))) => {
                if deleted_at.is_some() {
                    return Err(DomainError::Gone); // 帖子已经被删除
                }
            }
            Ok(None) => return Err(DomainError::NotFound), // 帖子不存在
            Err(e) => {
                let err_msg = e.to_string();
                if err_msg.contains("could not obtain lock") {
                    return Err(DomainError::Locked); // 帖子正在被操作
                }
                return Err(DomainError::Db(err_msg));
            }
        }

        // 4. 软删帖子
        let rows_affected = sqlx::query::<Postgres>(
            "UPDATE posts SET deleted_at = NOW() WHERE id = $1 AND deleted_at IS NULL"
        )
        .bind(post_id)
        .execute(tx.as_mut())
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?
        .rows_affected();

        if rows_affected == 0 {
            return Err(DomainError::Gone); // 帖子已被删除（并发情况）
        }

        // 5. 级联软删：删除帖子下的所有评论（一级和二级）
        sqlx::query::<Postgres>(
            "UPDATE comments SET deleted_at = NOW() WHERE post_id = $1 AND deleted_at IS NULL"
        )
        .bind(post_id)
        .execute(tx.as_mut())
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        // 6. 级联软删：删除帖子上的所有反应
        sqlx::query::<Postgres>(
            "UPDATE reactions SET deleted_at = NOW() WHERE resource_type = 1 AND resource_id = $1 AND deleted_at IS NULL"
        )
        .bind(post_id)
        .execute(tx.as_mut())
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        // 7. 级联软删：删除该帖子所有评论上的反应
        sqlx::query::<Postgres>(
            "UPDATE reactions SET deleted_at = NOW() WHERE resource_type = 2 AND resource_id IN (SELECT id FROM comments WHERE post_id = $1) AND deleted_at IS NULL"
        )
        .bind(post_id)
        .execute(tx.as_mut())
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        // 8. 发布删除事件
        sqlx::query::<Postgres>("SELECT pg_notify('events', $1)")
            .bind(format!(r#"{{"type":"post_deleted","id":{}}}"#, post_id))
            .execute(tx.as_mut())
            .await
            .map_err(|e| DomainError::Db(e.to_string()))?;

        // 9. 提交事务（带超时）
        tokio::time::timeout(
            Duration::from_secs(5),
            tx.commit()
        )
        .await
        .map_err(|_| DomainError::Db("事务提交超时".into()))?
        .map_err(|e| DomainError::Db(e.to_string()))?;

        Ok(())
    }

    // 软删除单条评论（一级评论或二级回复）
    pub async fn delete_comment_soft(&self, comment_id: i64, _actor_id: i64) -> Result<(), DomainError> {
        // 1. 开启事务（带超时）
        let mut tx = tokio::time::timeout(
            TRANSACTION_TIMEOUT,
            self.pool.begin()
        )
        .await
        .map_err(|_| DomainError::Db("事务开启超时".into()))?
        .map_err(|e| DomainError::Db(e.to_string()))?;

        // 2. 检查评论是否存在及其状态（使用 NOWAIT 避免阻塞）
        let comment_status = sqlx::query_as::<Postgres, (i64, Option<i64>, Option<DateTime<Utc>>)>(
            r#"SELECT post_id, parent_comment_id, deleted_at FROM comments WHERE id = $1 FOR UPDATE NOWAIT"#
        )
        .bind(comment_id)
        .fetch_optional(tx.as_mut())
        .await;

        let (post_id, parent_comment_id, deleted_at) = match comment_status {
            Ok(Some(info)) => info,
            Ok(None) => return Err(DomainError::NotFound), // 评论不存在
            Err(e) => {
                let err_msg = e.to_string();
                if err_msg.contains("could not obtain lock") {
                    return Err(DomainError::Locked); // 评论正在被操作
                }
                return Err(DomainError::Db(err_msg));
            }
        };

        if deleted_at.is_some() {
            return Err(DomainError::Gone); // 评论已经被删除
        }

        // 3. 使用帖子级别的顾问锁（带超时）
        self.advisory_lock_tx(&mut tx, post_id).await?;

        // 4. 软删除评论
        let rows_affected = sqlx::query::<Postgres>(
            "UPDATE comments SET deleted_at = NOW() WHERE id = $1 AND deleted_at IS NULL"
        )
        .bind(comment_id)
        .execute(tx.as_mut())
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?
        .rows_affected();

        if rows_affected == 0 {
            return Err(DomainError::Gone); // 评论已被删除（并发情况）
        }

        // 5. 如果是一级评论，级联软删除其下的所有二级回复
        if parent_comment_id.is_none() {
            sqlx::query::<Postgres>(
                "UPDATE comments SET deleted_at = NOW() WHERE parent_comment_id = $1 AND deleted_at IS NULL"
            )
            .bind(comment_id)
            .execute(tx.as_mut())
            .await
            .map_err(|e| DomainError::Db(e.to_string()))?;

            // 级联软删除二级回复上的反应
            sqlx::query::<Postgres>(
                "UPDATE reactions SET deleted_at = NOW() WHERE resource_type = 2 AND resource_id IN (SELECT id FROM comments WHERE parent_comment_id = $1) AND deleted_at IS NULL"
            )
            .bind(comment_id)
            .execute(tx.as_mut())
            .await
            .map_err(|e| DomainError::Db(e.to_string()))?;
        }

        // 6. 软删除该评论上的所有反应
        sqlx::query::<Postgres>(
            "UPDATE reactions SET deleted_at = NOW() WHERE resource_type = 2 AND resource_id = $1 AND deleted_at IS NULL"
        )
        .bind(comment_id)
        .execute(tx.as_mut())
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        // 7. 发布删除事件
        sqlx::query::<Postgres>("SELECT pg_notify('events', $1)")
            .bind(format!(r#"{{"type":"comment_deleted","id":{}}}"#, comment_id))
            .execute(tx.as_mut())
            .await
            .map_err(|e| DomainError::Db(e.to_string()))?;

        // 8. 提交事务（带超时）
        tokio::time::timeout(
            Duration::from_secs(5),
            tx.commit()
        )
        .await
        .map_err(|_| DomainError::Db("事务提交超时".into()))?
        .map_err(|e| DomainError::Db(e.to_string()))?;

        Ok(())
    }

    pub async fn react_idempotent(&self, resource_type: i16, resource_id: i64, reactor_id: i64, reaction_type: i16, idempotency_key: String) -> Result<(), DomainError> {
        // 1. 如果是收藏操作（reaction_type = 2），检查是否是作者自己
        if reaction_type == 2 {
            let author_id = if resource_type == 1 {
                // 检查帖子作者
                sqlx::query_as::<Postgres, (i64,)>(
                    r#"SELECT author_id FROM posts WHERE id = $1 AND deleted_at IS NULL"#
                )
                .bind(resource_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| DomainError::Db(e.to_string()))?
                .map(|(id,)| id)
            } else {
                // 检查评论作者
                sqlx::query_as::<Postgres, (i64,)>(
                    r#"SELECT author_id FROM comments WHERE id = $1 AND deleted_at IS NULL"#
                )
                .bind(resource_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| DomainError::Db(e.to_string()))?
                .map(|(id,)| id)
            };

            match author_id {
                None => return Err(DomainError::NotFound), // 资源不存在
                Some(author) => {
                    if author == reactor_id {
                        return Err(DomainError::Validation("不能收藏自己发布的内容".into()));
                    }
                }
            }
        }

        // 2. 开启事务（带超时）
        let mut tx = tokio::time::timeout(
            TRANSACTION_TIMEOUT,
            self.pool.begin()
        )
        .await
        .map_err(|_| DomainError::Db("事务开启超时".into()))?
        .map_err(|e| DomainError::Db(e.to_string()))?;

        // 3. 针对帖子使用顾问锁，评论可按分桶hash锁
        let lock_key = if resource_type == 1 { resource_id } else { (resource_id % 1024) as i64 };
        self.advisory_lock_tx(&mut tx, lock_key).await?;

        // 4. 检查资源是否已删除
        let resource_exists = if resource_type == 1 {
            // 检查帖子
            sqlx::query_as::<Postgres, (Option<DateTime<Utc>>,)>(
                r#"SELECT deleted_at FROM posts WHERE id = $1"#
            )
            .bind(resource_id)
            .fetch_optional(tx.as_mut())
            .await
            .map_err(|e| DomainError::Db(e.to_string()))?
        } else {
            // 检查评论
            sqlx::query_as::<Postgres, (Option<DateTime<Utc>>,)>(
                r#"SELECT deleted_at FROM comments WHERE id = $1"#
            )
            .bind(resource_id)
            .fetch_optional(tx.as_mut())
            .await
            .map_err(|e| DomainError::Db(e.to_string()))?
        };

        match resource_exists {
            None => return Err(DomainError::NotFound), // 资源不存在
            Some((deleted_at,)) => {
                if deleted_at.is_some() {
                    return Err(DomainError::Gone); // 资源已删除
                }
            }
        }

        // 5. 幂等插入反应
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

        // 6. 发布事件通知
        sqlx::query::<Postgres>("SELECT pg_notify('events', $1)")
            .bind(format!(r#"{{"type":"reaction","rid":{},"rt":{}}}"#, resource_id, reaction_type))
        .execute(tx.as_mut())
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        // 7. 提交事务（带超时）
        tokio::time::timeout(
            Duration::from_secs(5),
            tx.commit()
        )
        .await
        .map_err(|_| DomainError::Db("事务提交超时".into()))?
        .map_err(|e| DomainError::Db(e.to_string()))?;

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

    // 检查帖子状态（用于前端验证帖子是否存在）
    pub async fn check_post_status(&self, post_id: i64) -> Result<PostStatus, DomainError> {
        let result = sqlx::query_as::<Postgres, (Option<DateTime<Utc>>, Option<DateTime<Utc>>)>(
            r#"SELECT locked_at, deleted_at FROM posts WHERE id = $1"#
        )
        .bind(post_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        match result {
            None => Err(DomainError::NotFound), // 帖子不存在
            Some((locked_at, deleted_at)) => {
                if deleted_at.is_some() {
                    Err(DomainError::Gone) // 帖子已删除
                } else if locked_at.is_some() {
                    Ok(PostStatus::Locked) // 帖子已锁定
                } else {
                    Ok(PostStatus::Active) // 帖子正常
                }
            }
        }
    }

    // 获取帖子的评论树（一级评论 + 二级回复）
    // 按最新时间排序（降序，最新的在前面）
    pub async fn get_comments_tree(&self, post_id: i64) -> Result<Vec<(CommentRow, Vec<CommentRow>)>, DomainError> {
        // 获取所有一级评论（parent_comment_id IS NULL）
        // 按创建时间降序排列，最新的在前面
        let parent_comments = sqlx::query_as::<Postgres, CommentRow>(
            r#"SELECT id, post_id, author_id, parent_comment_id, content, at_user_id, deleted_at, created_at
               FROM comments
               WHERE post_id = $1 AND parent_comment_id IS NULL AND deleted_at IS NULL
               ORDER BY created_at DESC"#
        )
        .bind(post_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        if parent_comments.is_empty() {
            return Ok(vec![]);
        }

        // 获取所有二级回复
        let parent_ids: Vec<i64> = parent_comments.iter().map(|c| c.id).collect();
        let replies = sqlx::query_as::<Postgres, CommentRow>(
            r#"SELECT id, post_id, author_id, parent_comment_id, content, at_user_id, deleted_at, created_at
               FROM comments
               WHERE post_id = $1 AND parent_comment_id = ANY($2) AND deleted_at IS NULL
               ORDER BY created_at DESC"#
        )
        .bind(post_id)
        .bind(&parent_ids)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DomainError::Db(e.to_string()))?;

        // 构建树形结构
        let mut tree: Vec<(CommentRow, Vec<CommentRow>)> = Vec::new();
        for parent in parent_comments {
            let parent_id = parent.id;
            let children: Vec<CommentRow> = replies
                .iter()
                .filter(|r| r.parent_comment_id == Some(parent_id))
                .cloned()
                .collect();
            tree.push((parent, children));
        }

        Ok(tree)
    }
}