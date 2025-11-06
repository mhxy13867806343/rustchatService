use thiserror::Error;

#[derive(Debug, Error)]
pub enum DomainError {
    #[error("资源未找到")] 
    NotFound,
    #[error("资源已删除")] 
    Gone,
    #[error("资源被锁定")] 
    Locked,
    #[error("请求过于频繁")] 
    TooManyRequests,
    #[error("参数校验失败: {0}")] 
    Validation(String),
    #[error("数据库错误: {0}")] 
    Db(String),
}

impl DomainError {
    // 映射到语义化状态码（非HTTP，仅供上层使用）
    pub fn code(&self) -> u16 {
        match self {
            DomainError::NotFound => 404,
            DomainError::Gone => 410,
            DomainError::Locked => 423,
            DomainError::TooManyRequests => 429,
            DomainError::Validation(_) => 422,
            DomainError::Db(_) => 500,
        }
    }
}