"""
FastAPI 集成示例
展示如何在 FastAPI 项目中集成 Rust 聊天服务
"""

from fastapi import FastAPI, HTTPException, Query
from pydantic import BaseModel
from typing import Optional, List
from python_client_example import RustChatClient

app = FastAPI(title="Python FastAPI + Rust Chat Service")

# 初始化 Rust 聊天服务客户端
rust_client = RustChatClient(
    base_url="http://127.0.0.1:8081",
    auth_secret="sso-secret"  # 与 Rust 服务的 AUTH_SECRET 保持一致
)


# ==================== 数据模型 ====================

class MessageRequest(BaseModel):
    room_id: str
    username: str
    content: str


class CommentRequest(BaseModel):
    post_id: int
    author_id: int
    content: str
    parent_comment_id: Optional[int] = None
    at_user_id: Optional[int] = None


class ReactionRequest(BaseModel):
    resource_type: int  # 1=post, 2=comment
    resource_id: int
    reactor_id: int
    reaction_type: int  # 1=like, 2=favorite


class SocialActionRequest(BaseModel):
    action: str  # follow | unfollow | block | unblock | mute | unmute
    target: str


# ==================== API 端点 ====================

@app.get("/")
async def root():
    """根路径"""
    return {
        "service": "Python FastAPI + Rust Chat Service",
        "status": "running",
        "rust_service": rust_client.base_url
    }


@app.post("/api/messages/send")
async def send_message(request: MessageRequest):
    """发送消息到聊天室"""
    success = rust_client.publish_message(
        request.room_id,
        request.username,
        request.content
    )
    if not success:
        raise HTTPException(status_code=500, detail="Failed to send message")
    return {"status": "ok", "message": "Message sent successfully"}


@app.get("/api/rooms/{room_id}/users")
async def get_room_users(room_id: str):
    """获取房间用户列表"""
    users = rust_client.get_room_users(room_id)
    return {"status": "ok", "users": users}


@app.get("/api/rooms/{room_id}/search")
async def search_room_users(room_id: str, q: str = Query(..., description="搜索关键字")):
    """搜索房间用户"""
    users = rust_client.search_users(room_id, q)
    return {"status": "ok", "users": users}


@app.post("/api/comments/create")
async def create_comment(request: CommentRequest):
    """创建评论
    
    - 一级评论：parent_comment_id 为 None
    - 二级回复：parent_comment_id 为父评论的 ID
    - 可选 @某人：设置 at_user_id
    """
    comment = rust_client.create_comment(
        post_id=request.post_id,
        author_id=request.author_id,
        content=request.content,
        parent_comment_id=request.parent_comment_id,
        at_user_id=request.at_user_id
    )
    if not comment:
        raise HTTPException(status_code=500, detail="Failed to create comment")
    return {"status": "ok", "comment": comment}


@app.get("/api/posts/{post_id}/status")
async def check_post_status(post_id: int):
    """检查帖子状态（用于前端验证）
    
    返回：
    - exists: 帖子是否存在
    - deleted: 帖子是否已删除
    - locked: 帖子是否已锁定
    - message: 状态描述
    """
    status = rust_client.check_post_status(post_id)
    
    # 根据状态返回不同的 HTTP 状态码
    if not status.get('exists'):
        raise HTTPException(status_code=404, detail=status.get('message', '帖子不存在'))
    
    if status.get('deleted'):
        raise HTTPException(status_code=410, detail=status.get('message', '帖子已被删除'))
    
    return {"status": "ok", "data": status}


@app.get("/api/posts/{post_id}/comments")
async def get_post_comments(post_id: int):
    """获取帖子的评论列表（嵌套结构）
    
    注意：此接口会自动检查帖子状态
    
    返回格式：
    [
        {
            "id": 1,
            "content": "一级评论",
            "replies": [
                {"id": 2, "content": "二级回复", "at_user_id": 100}
            ]
        }
    ]
    """
    # 先检查帖子状态
    status = rust_client.check_post_status(post_id)
    
    if not status.get('exists'):
        raise HTTPException(status_code=404, detail="帖子不存在")
    
    if status.get('deleted'):
        raise HTTPException(status_code=410, detail="帖子已被删除")
    
    # 帖子正常，获取评论
    comments = rust_client.get_comments(post_id)
    return {"status": "ok", "comments": comments, "post_locked": status.get('locked', False)}


@app.delete("/api/posts/{post_id}")
async def delete_post(post_id: int):
    """删除帖子（软删除，级联删除所有评论和反应）"""
    success = rust_client.delete_post(post_id)
    if not success:
        raise HTTPException(status_code=500, detail="Failed to delete post")
    return {"status": "ok", "message": "Post and all comments deleted successfully"}


@app.delete("/api/comments/{comment_id}")
async def delete_comment(comment_id: int):
    """删除评论（软删除）
    
    - 如果是一级评论，会级联删除其下的所有二级回复
    - 如果是二级回复，只删除该回复本身
    """
    success = rust_client.delete_comment(comment_id)
    if not success:
        raise HTTPException(status_code=500, detail="Failed to delete comment")
    return {"status": "ok", "message": "Comment deleted successfully"}


@app.post("/api/reactions/add")
async def add_reaction(request: ReactionRequest):
    """添加反应（点赞/收藏）"""
    success = rust_client.add_reaction(
        resource_type=request.resource_type,
        resource_id=request.resource_id,
        reactor_id=request.reactor_id,
        reaction_type=request.reaction_type
    )
    if not success:
        raise HTTPException(status_code=500, detail="Failed to add reaction")
    return {"status": "ok", "message": "Reaction added successfully"}


@app.post("/api/social/action")
async def social_action(request: SocialActionRequest):
    """执行社交操作"""
    success = rust_client.social_action(request.action, request.target)
    if not success:
        raise HTTPException(status_code=500, detail="Failed to perform social action")
    return {"status": "ok", "message": f"Action '{request.action}' performed successfully"}


# ==================== 业务逻辑示例 ====================

@app.post("/api/posts/{post_id}/like")
async def like_post(post_id: int, user_id: int):
    """点赞帖子（业务封装）"""
    success = rust_client.add_reaction(
        resource_type=1,  # 1=post
        resource_id=post_id,
        reactor_id=user_id,
        reaction_type=1  # 1=like
    )
    if not success:
        raise HTTPException(status_code=500, detail="Failed to like post")
    return {"status": "ok", "message": "Post liked"}


@app.post("/api/comments/{comment_id}/like")
async def like_comment(comment_id: int, user_id: int):
    """点赞评论（业务封装）"""
    success = rust_client.add_reaction(
        resource_type=2,  # 2=comment
        resource_id=comment_id,
        reactor_id=user_id,
        reaction_type=1  # 1=like
    )
    if not success:
        raise HTTPException(status_code=500, detail="Failed to like comment")
    return {"status": "ok", "message": "Comment liked"}


@app.post("/api/posts/{post_id}/reply")
async def reply_to_post(
    post_id: int,
    author_id: int,
    content: str,
    at_author: bool = False
):
    """回复帖子（创建一级评论）
    
    Args:
        post_id: 帖子ID
        author_id: 回复者ID
        content: 回复内容
        at_author: 是否 @帖子作者（需要传入作者ID）
    """
    # 这里简化处理，实际应该从数据库获取帖子作者ID
    at_user_id = None  # 如果 at_author=True，这里应该设置为帖子作者的ID
    
    comment = rust_client.create_comment(
        post_id=post_id,
        author_id=author_id,
        content=content,
        parent_comment_id=None,  # 一级评论
        at_user_id=at_user_id
    )
    if not comment:
        raise HTTPException(status_code=500, detail="Failed to reply to post")
    return {"status": "ok", "comment": comment}


@app.post("/api/comments/{comment_id}/reply")
async def reply_to_comment(
    comment_id: int,
    post_id: int,
    author_id: int,
    content: str,
    at_comment_author_id: Optional[int] = None
):
    """回复评论（创建二级回复）
    
    Args:
        comment_id: 被回复的评论ID（一级评论）
        post_id: 帖子ID
        author_id: 回复者ID
        content: 回复内容
        at_comment_author_id: @被回复评论的作者ID
    """
    comment = rust_client.create_comment(
        post_id=post_id,
        author_id=author_id,
        content=content,
        parent_comment_id=comment_id,  # 二级回复
        at_user_id=at_comment_author_id
    )
    if not comment:
        raise HTTPException(status_code=500, detail="Failed to reply to comment")
    return {"status": "ok", "comment": comment}


# ==================== 启动说明 ====================

if __name__ == "__main__":
    import uvicorn
    
    print("=" * 60)
    print("FastAPI + Rust Chat Service 集成示例")
    print("=" * 60)
    print("\n启动步骤：")
    print("1. 确保 Rust 聊天服务已启动（http://127.0.0.1:8081）")
    print("2. 运行此 FastAPI 服务：")
    print("   uvicorn fastapi_integration_example:app --reload --port 8000")
    print("\n访问地址：")
    print("- API 文档: http://127.0.0.1:8000/docs")
    print("- ReDoc: http://127.0.0.1:8000/redoc")
    print("=" * 60)
    
    uvicorn.run(app, host="0.0.0.0", port=8000)
