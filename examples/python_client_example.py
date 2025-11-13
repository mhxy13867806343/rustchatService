"""
Python FastAPI 客户端示例
展示如何从 Python 调用 Rust 聊天服务的 API
"""

import hashlib
import hmac
import time
import uuid
import requests
from typing import Optional


class RustChatClient:
    """Rust 聊天服务客户端"""
    
    def __init__(self, base_url: str = "http://127.0.0.1:8081", auth_secret: str = "sso-secret"):
        self.base_url = base_url
        self.auth_secret = auth_secret
        self.jwt_token: Optional[str] = None
    
    def _generate_uid_hash(self) -> str:
        """生成 36 位字母数字的 uid_hash"""
        return str(uuid.uuid4()).replace("-", "")
    
    def _generate_signature(self, canonical: str) -> str:
        """生成 HMAC-SHA256 签名"""
        mac = hmac.new(
            self.auth_secret.encode('utf-8'),
            canonical.encode('utf-8'),
            hashlib.sha256
        )
        return mac.hexdigest()
    
    def _get_auth_params(self, **params) -> dict:
        """生成认证参数"""
        ts = int(time.time())
        nonce = str(uuid.uuid4())
        uid_hash = self._generate_uid_hash()
        
        # 构建规范字符串（按字母顺序排列参数）
        sorted_params = sorted(params.items())
        canonical_parts = [f"{k}={v}" for k, v in sorted_params]
        canonical_parts.extend([
            f"ts={ts}",
            f"nonce={nonce}",
            f"uid_hash={uid_hash}"
        ])
        canonical = "&".join(canonical_parts)
        
        sig = self._generate_signature(canonical)
        
        return {
            "ts": ts,
            "nonce": nonce,
            "uid_hash": uid_hash,
            "sig": sig
        }
    
    def _get_headers(self) -> dict:
        """获取请求头"""
        headers = {"Content-Type": "application/json"}
        if self.jwt_token:
            headers["Authorization"] = f"Bearer {self.jwt_token}"
        return headers
    
    def login(self, username: str = "py-bot", password: str = "password") -> bool:
        """登录获取 JWT Token"""
        url = f"{self.base_url}/auth/login"
        data = {"username": username, "password": password}
        
        try:
            response = requests.post(url, json=data)
            response.raise_for_status()
            result = response.json()
            if result.get("code") == 0:
                self.jwt_token = result["data"]["token"]
                print(f"✓ 登录成功，获取到 JWT Token")
                return True
            else:
                print(f"✗ 登录失败: {result.get('message')}")
                return False
        except Exception as e:
            print(f"✗ 登录异常: {e}")
            return False
    
    def publish_message(self, room_id: str, username: str, content: str) -> bool:
        """发布消息到聊天室"""
        url = f"{self.base_url}/api/rooms/{room_id}/publish"
        
        auth_params = self._get_auth_params(
            room_id=room_id,
            username=username,
            content=content
        )
        
        data = {"username": username, "content": content}
        
        try:
            response = requests.post(
                url,
                json=data,
                params=auth_params,
                headers=self._get_headers()
            )
            response.raise_for_status()
            result = response.json()
            if result.get("code") == 0:
                print(f"✓ 消息发布成功: {content}")
                return True
            else:
                print(f"✗ 消息发布失败: {result.get('message')}")
                return False
        except Exception as e:
            print(f"✗ 消息发布异常: {e}")
            return False
    
    def get_room_users(self, room_id: str) -> list:
        """获取房间用户列表"""
        url = f"{self.base_url}/api/rooms/{room_id}/users"
        
        auth_params = self._get_auth_params(room_id=room_id)
        
        try:
            response = requests.get(
                url,
                params=auth_params,
                headers=self._get_headers()
            )
            response.raise_for_status()
            result = response.json()
            if result.get("code") == 0:
                users = result.get("data", [])
                print(f"✓ 获取房间用户成功: {users}")
                return users
            else:
                print(f"✗ 获取房间用户失败: {result.get('message')}")
                return []
        except Exception as e:
            print(f"✗ 获取房间用户异常: {e}")
            return []
    
    def search_users(self, room_id: str, query: str) -> list:
        """搜索房间用户"""
        url = f"{self.base_url}/api/rooms/{room_id}/search"
        
        auth_params = self._get_auth_params(room_id=room_id, q=query)
        auth_params["q"] = query
        
        try:
            response = requests.get(
                url,
                params=auth_params,
                headers=self._get_headers()
            )
            response.raise_for_status()
            result = response.json()
            if result.get("code") == 0:
                users = result.get("data", [])
                print(f"✓ 搜索用户成功: {users}")
                return users
            else:
                print(f"✗ 搜索用户失败: {result.get('message')}")
                return []
        except Exception as e:
            print(f"✗ 搜索用户异常: {e}")
            return []
    
    def social_action(self, action: str, target: str) -> bool:
        """执行社交操作（follow/unfollow/block/unblock/mute/unmute）"""
        url = f"{self.base_url}/api/social/action"
        
        auth_params = self._get_auth_params(action=action, target=target)
        data = {"action": action, "target": target}
        
        try:
            response = requests.post(
                url,
                json=data,
                params=auth_params,
                headers=self._get_headers()
            )
            response.raise_for_status()
            result = response.json()
            if result.get("code") == 0:
                print(f"✓ 社交操作成功: {action} {target}")
                return True
            else:
                print(f"✗ 社交操作失败: {result.get('message')}")
                return False
        except Exception as e:
            print(f"✗ 社交操作异常: {e}")
            return False
    
    def create_comment(self, post_id: int, author_id: int, content: str, 
                      parent_comment_id: Optional[int] = None,
                      at_user_id: Optional[int] = None) -> Optional[dict]:
        """创建评论
        
        Args:
            post_id: 帖子ID
            author_id: 作者ID
            content: 评论内容
            parent_comment_id: 父评论ID（一级评论为None，二级回复填父评论ID）
            at_user_id: @的用户ID（可选）
        """
        url = f"{self.base_url}/api/comments"
        
        idempotency_key = str(uuid.uuid4())
        
        auth_params = self._get_auth_params(
            post_id=post_id,
            author_id=author_id,
            content=content
        )
        
        data = {
            "post_id": post_id,
            "author_id": author_id,
            "content": content,
            "parent_comment_id": parent_comment_id,
            "at_user_id": at_user_id,
            "idempotency_key": idempotency_key
        }
        
        try:
            response = requests.post(
                url,
                json=data,
                params=auth_params,
                headers=self._get_headers()
            )
            response.raise_for_status()
            result = response.json()
            if result.get("code") == 0:
                comment = result.get("data")
                print(f"✓ 评论创建成功: ID={comment.get('id')}")
                return comment
            else:
                print(f"✗ 评论创建失败: {result.get('message')}")
                return None
        except Exception as e:
            print(f"✗ 评论创建异常: {e}")
            return None
    
    def check_post_status(self, post_id: int) -> dict:
        """检查帖子状态（用于前端验证帖子是否存在）
        
        返回格式：
        {
            "exists": true/false,      # 帖子是否存在
            "deleted": true/false,     # 帖子是否已删除
            "locked": true/false,      # 帖子是否已锁定
            "message": "状态描述"
        }
        """
        url = f"{self.base_url}/api/posts/{post_id}/status"
        
        auth_params = self._get_auth_params(post_id=post_id)
        
        try:
            response = requests.get(
                url,
                params=auth_params,
                headers=self._get_headers()
            )
            result = response.json()
            
            if result.get("code") == 0:
                # 帖子正常
                status = result.get("data", {})
                print(f"✓ 帖子状态: {status.get('message')}")
                return status
            elif result.get("code") == 404:
                # 帖子不存在
                status = result.get("data", {})
                print(f"✗ {status.get('message', '帖子不存在')}")
                return status
            elif result.get("code") == 410:
                # 帖子已删除
                status = result.get("data", {})
                print(f"✗ {status.get('message', '帖子已被删除')}")
                return status
            else:
                print(f"✗ 检查帖子状态失败: {result.get('message')}")
                return {"exists": False, "deleted": False, "locked": False, "message": "未知错误"}
        except Exception as e:
            print(f"✗ 检查帖子状态异常: {e}")
            return {"exists": False, "deleted": False, "locked": False, "message": str(e)}
    
    def get_comments(self, post_id: int) -> list:
        """获取帖子的评论列表（嵌套结构）
        
        返回格式：
        [
            {
                "id": 1,
                "post_id": 1,
                "author_id": 100,
                "content": "一级评论",
                "at_user_id": null,
                "created_at": "2024-01-01T00:00:00Z",
                "replies": [
                    {
                        "id": 2,
                        "author_id": 101,
                        "content": "二级回复",
                        "at_user_id": 100,
                        "created_at": "2024-01-01T00:01:00Z"
                    }
                ]
            }
        ]
        """
        url = f"{self.base_url}/api/posts/{post_id}/comments"
        
        auth_params = self._get_auth_params(post_id=post_id)
        
        try:
            response = requests.get(
                url,
                params=auth_params,
                headers=self._get_headers()
            )
            response.raise_for_status()
            result = response.json()
            if result.get("code") == 0:
                comments = result.get("data", [])
                print(f"✓ 获取评论成功: 共 {len(comments)} 条一级评论")
                return comments
            else:
                print(f"✗ 获取评论失败: {result.get('message')}")
                return []
        except Exception as e:
            print(f"✗ 获取评论异常: {e}")
            return []
    
    def delete_post(self, post_id: int) -> bool:
        """删除帖子（软删除，会级联删除所有评论和反应）"""
        url = f"{self.base_url}/api/posts/{post_id}"
        
        auth_params = self._get_auth_params(post_id=post_id)
        
        try:
            response = requests.delete(
                url,
                params=auth_params,
                headers=self._get_headers()
            )
            if response.status_code == 410:
                print(f"✗ 帖子已被删除: ID={post_id}")
                return False
            response.raise_for_status()
            result = response.json()
            if result.get("code") == 0:
                print(f"✓ 帖子删除成功: ID={post_id} - {result.get('message')}")
                return True
            else:
                print(f"✗ 帖子删除失败: {result.get('message')}")
                return False
        except Exception as e:
            print(f"✗ 帖子删除异常: {e}")
            return False
    
    def delete_comment(self, comment_id: int) -> bool:
        """删除评论（软删除）
        
        - 如果是一级评论，会级联删除其下的所有二级回复
        - 如果是二级回复，只删除该回复
        """
        url = f"{self.base_url}/api/comments/{comment_id}"
        
        auth_params = self._get_auth_params(comment_id=comment_id)
        
        try:
            response = requests.delete(
                url,
                params=auth_params,
                headers=self._get_headers()
            )
            if response.status_code == 410:
                print(f"✗ 评论已被删除: ID={comment_id}")
                return False
            response.raise_for_status()
            result = response.json()
            if result.get("code") == 0:
                print(f"✓ 评论删除成功: ID={comment_id} - {result.get('message')}")
                return True
            else:
                print(f"✗ 评论删除失败: {result.get('message')}")
                return False
        except Exception as e:
            print(f"✗ 评论删除异常: {e}")
            return False
    
    def add_reaction(self, resource_type: int, resource_id: int, 
                    reactor_id: int, reaction_type: int) -> bool:
        """添加反应（点赞/收藏）"""
        url = f"{self.base_url}/api/reactions"
        
        idempotency_key = str(uuid.uuid4())
        
        auth_params = self._get_auth_params(
            resource_type=resource_type,
            resource_id=resource_id,
            reactor_id=reactor_id,
            reaction_type=reaction_type
        )
        
        data = {
            "resource_type": resource_type,
            "resource_id": resource_id,
            "reactor_id": reactor_id,
            "reaction_type": reaction_type,
            "idempotency_key": idempotency_key
        }
        
        try:
            response = requests.post(
                url,
                json=data,
                params=auth_params,
                headers=self._get_headers()
            )
            response.raise_for_status()
            result = response.json()
            if result.get("code") == 0:
                print(f"✓ 反应添加成功")
                return True
            else:
                print(f"✗ 反应添加失败: {result.get('message')}")
                return False
        except Exception as e:
            print(f"✗ 反应添加异常: {e}")
            return False


def main():
    """示例：演示如何使用客户端"""
    print("=" * 60)
    print("Rust 聊天服务 Python 客户端示例")
    print("=" * 60)
    
    # 创建客户端（使用默认密钥）
    client = RustChatClient(
        base_url="http://127.0.0.1:8081",
        auth_secret="sso-secret"  # 与 Rust 服务的 AUTH_SECRET 保持一致
    )
    
    print("\n1. 测试登录（可选，获取 JWT Token）")
    client.login("py-bot", "password")
    
    print("\n2. 测试发布消息")
    client.publish_message("room-001", "python-user", "Hello from Python!")
    
    print("\n3. 测试获取房间用户")
    client.get_room_users("room-001")
    
    print("\n4. 测试搜索用户")
    client.search_users("room-001", "python")
    
    print("\n5. 测试社交操作")
    client.social_action("follow", "target-user")
    
    print("\n6. 测试创建一级评论")
    comment1 = client.create_comment(
        post_id=1,
        author_id=100,
        content="这是一条来自 Python 的一级评论"
    )
    
    print("\n7. 测试创建二级回复（回复一级评论）")
    if comment1:
        client.create_comment(
            post_id=1,
            author_id=101,
            content="这是对一级评论的回复",
            parent_comment_id=comment1.get("id"),
            at_user_id=100  # @一级评论的作者
        )
    
    print("\n8. 测试获取评论列表（嵌套结构）")
    comments = client.get_comments(post_id=1)
    if comments:
        print(f"\n评论树结构示例：")
        for i, comment in enumerate(comments[:2], 1):  # 只显示前2条
            print(f"  [{i}] 一级评论 (ID={comment['id']}, 作者={comment['author_id']}): {comment['content']}")
            for j, reply in enumerate(comment.get('replies', []), 1):
                at_info = f" @{reply['at_user_id']}" if reply.get('at_user_id') else ""
                print(f"      └─ [{j}] 回复 (ID={reply['id']}, 作者={reply['author_id']}{at_info}): {reply['content']}")
    
    print("\n9. 测试添加反应（点赞）")
    client.add_reaction(
        resource_type=1,  # 1=post
        resource_id=1,
        reactor_id=100,
        reaction_type=1  # 1=like
    )
    
    print("\n" + "=" * 60)
    print("测试完成！")
    print("=" * 60)


if __name__ == "__main__":
    main()
