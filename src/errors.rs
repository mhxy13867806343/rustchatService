use thiserror::Error;

#[derive(Debug, Error)]
pub enum DomainError {
    #[error("资源未找到")] 
    NotFound,
    #[error("资源已删除，无法操作")] 
    Gone,
    #[error("资源正在被操作，请稍后重试")] 
    Locked,
    #[error("请求过于频繁，请稍后再试（建议间隔3秒以上）")] 
    TooManyRequests,
    #[error("参数校验失败: {0}")] 
    Validation(String),
    #[error("数据库错误: {0}")] 
    Db(String),
    #[error("操作超时，请重试")] 
    Timeout,
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
            DomainError::Timeout => 408,
            DomainError::Db(_) => 500,
        }
    }
}