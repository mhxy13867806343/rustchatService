"""
评论功能测试脚本
测试嵌套评论结构：一级评论 + 二级回复 + @功能
"""

from python_client_example import RustChatClient
import time

def test_comments():
    """测试评论功能"""
    print("=" * 70)
    print("评论功能测试")
    print("=" * 70)
    
    # 创建客户端
    client = RustChatClient(
        base_url="http://127.0.0.1:8081",
        auth_secret="sso-secret"
    )
    
    # 测试帖子ID
    post_id = 1
    
    print(f"\n📝 测试帖子 ID: {post_id}")
    print("-" * 70)
    
    # 1. 创建第一条一级评论
    print("\n1️⃣  创建第一条一级评论（作者ID=100）")
    comment1 = client.create_comment(
        post_id=post_id,
        author_id=100,
        content="这是第一条一级评论，讨论一下这个话题"
    )
    time.sleep(0.5)
    
    # 2. 创建第二条一级评论
    print("\n2️⃣  创建第二条一级评论（作者ID=101）")
    comment2 = client.create_comment(
        post_id=post_id,
        author_id=101,
        content="我也来发表一下看法"
    )
    time.sleep(0.5)
    
    # 3. 回复第一条评论（不@）
    if comment1:
        print("\n3️⃣  回复第一条评论（作者ID=102，不@）")
        client.create_comment(
            post_id=post_id,
            author_id=102,
            content="我同意你的观点",
            parent_comment_id=comment1["id"]
        )
        time.sleep(0.5)
    
    # 4. 回复第一条评论（@原作者）
    if comment1:
        print("\n4️⃣  回复第一条评论（作者ID=103，@原作者100）")
        client.create_comment(
            post_id=post_id,
            author_id=103,
            content="@100 你说得对，我补充一点",
            parent_comment_id=comment1["id"],
            at_user_id=100
        )
        time.sleep(0.5)
    
    # 5. 回复第二条评论
    if comment2:
        print("\n5️⃣  回复第二条评论（作者ID=104，@原作者101）")
        client.create_comment(
            post_id=post_id,
            author_id=104,
            content="@101 能详细说说吗？",
            parent_comment_id=comment2["id"],
            at_user_id=101
        )
        time.sleep(0.5)
    
    # 6. 再给第一条评论添加一个回复
    if comment1:
        print("\n6️⃣  再给第一条评论添加回复（作者ID=105）")
        client.create_comment(
            post_id=post_id,
            author_id=105,
            content="我也有同样的想法",
            parent_comment_id=comment1["id"]
        )
        time.sleep(0.5)
    
    # 7. 获取完整的评论树
    print("\n" + "=" * 70)
    print("📋 获取完整的评论树结构")
    print("=" * 70)
    
    comments = client.get_comments(post_id)
    
    if comments:
        print(f"\n共有 {len(comments)} 条一级评论\n")
        
        for i, comment in enumerate(comments, 1):
            # 显示一级评论
            print(f"┌─ [{i}] 一级评论 (ID={comment['id']}, 作者={comment['author_id']})")
            print(f"│   内容: {comment['content']}")
            print(f"│   时间: {comment['created_at']}")
            
            # 显示二级回复
            replies = comment.get('replies', [])
            if replies:
                print(f"│   └─ 共 {len(replies)} 条回复:")
                for j, reply in enumerate(replies, 1):
                    at_info = f" @{reply['at_user_id']}" if reply.get('at_user_id') else ""
                    print(f"│      ├─ [{j}] 回复 (ID={reply['id']}, 作者={reply['author_id']}{at_info})")
                    print(f"│      │   内容: {reply['content']}")
                    print(f"│      │   时间: {reply['created_at']}")
            else:
                print(f"│   └─ 暂无回复")
            
            print("│")
        
        print("└" + "─" * 68)
    else:
        print("\n暂无评论")
    
    # 8. 测试点赞功能
    print("\n" + "=" * 70)
    print("👍 测试点赞功能")
    print("=" * 70)
    
    if comment1:
        print(f"\n给一级评论 {comment1['id']} 点赞")
        client.add_reaction(
            resource_type=2,  # 2=comment
            resource_id=comment1['id'],
            reactor_id=200,
            reaction_type=1  # 1=like
        )
    
    print("\n" + "=" * 70)
    print("✅ 测试完成！")
    print("=" * 70)
    
    # 返回数据结构示例
    print("\n📊 数据结构说明：")
    print("""
    返回的评论树结构：
    [
        {
            "id": 1,                    # 一级评论ID
            "post_id": 1,               # 帖子ID
            "author_id": 100,           # 作者ID
            "content": "评论内容",       # 评论内容
            "at_user_id": null,         # @的用户ID（一级评论通常为null）
            "created_at": "2024-...",   # 创建时间
            "replies": [                # 二级回复列表
                {
                    "id": 2,            # 回复ID
                    "author_id": 102,   # 回复者ID
                    "content": "回复内容",
                    "at_user_id": 100,  # @的用户ID
                    "created_at": "2024-..."
                }
            ]
        }
    ]
    
    特点：
    - 最多支持二层结构（一级评论 + 二级回复）
    - 二级回复可以 @任何用户（通常是一级评论作者或帖子作者）
    - 按创建时间升序排列
    - 支持幂等性（相同的 idempotency_key 不会重复创建）
    """)


if __name__ == "__main__":
    test_comments()
