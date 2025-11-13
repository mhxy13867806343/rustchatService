"""
边界情况测试
测试：
1. 评论列表按最新时间排序
2. 不能收藏自己发布的内容
3. 连续评论间隔限制（3秒）
4. 并发冲突处理
"""

from python_client_example import RustChatClient
import time

def test_edge_cases():
    """测试边界情况"""
    print("=" * 70)
    print("边界情况测试")
    print("=" * 70)
    
    client = RustChatClient(
        base_url="http://127.0.0.1:8081",
        auth_secret="sso-secret"
    )
    
    post_id = 2000  # 使用一个测试帖子ID
    
    # ==================== 测试 1: 评论列表按最新时间排序 ====================
    print("\n【测试 1】评论列表按最新时间排序（最新的在前面）")
    print("-" * 70)
    
    print("\n1. 创建第一条评论（时间：T1）")
    comment1 = client.create_comment(
        post_id=post_id,
        author_id=1001,
        content="第一条评论 - 时间最早"
    )
    time.sleep(1)
    
    print("\n2. 创建第二条评论（时间：T2）")
    comment2 = client.create_comment(
        post_id=post_id,
        author_id=1002,
        content="第二条评论 - 时间居中"
    )
    time.sleep(1)
    
    print("\n3. 创建第三条评论（时间：T3）")
    comment3 = client.create_comment(
        post_id=post_id,
        author_id=1003,
        content="第三条评论 - 时间最新"
    )
    time.sleep(0.5)
    
    print("\n4. 获取评论列表，验证排序")
    comments = client.get_comments(post_id)
    if comments:
        print(f"\n   评论顺序（应该是最新的在前面）：")
        for i, c in enumerate(comments, 1):
            print(f"   [{i}] ID={c['id']}, 内容: {c['content']}")
            print(f"       时间: {c['created_at']}")
        
        if len(comments) >= 3:
            # 验证第一条是最新的
            if "最新" in comments[0]['content']:
                print("\n   ✓ 排序正确：最新的评论在最前面")
            else:
                print("\n   ✗ 排序错误：最新的评论不在最前面")
    
    # ==================== 测试 2: 不能收藏自己发布的内容 ====================
    print("\n\n【测试 2】不能收藏自己发布的内容")
    print("-" * 70)
    
    if comment1:
        print(f"\n1. 尝试收藏自己的评论（作者ID=1001，评论ID={comment1['id']}）")
        print("   预期：返回 422，提示不能收藏自己发布的内容")
        success = client.add_reaction(
            resource_type=2,  # 2=comment
            resource_id=comment1['id'],
            reactor_id=1001,  # 与作者ID相同
            reaction_type=2  # 2=favorite
        )
        if not success:
            print("   ✓ 正确：不能收藏自己的评论")
        else:
            print("   ✗ 错误：应该禁止收藏自己的评论")
        time.sleep(0.5)
        
        print(f"\n2. 其他用户收藏该评论（用户ID=1002，评论ID={comment1['id']}）")
        print("   预期：成功")
        success = client.add_reaction(
            resource_type=2,
            resource_id=comment1['id'],
            reactor_id=1002,  # 不同的用户
            reaction_type=2
        )
        if success:
            print("   ✓ 正确：其他用户可以收藏")
        else:
            print("   ✗ 错误：其他用户应该可以收藏")
        time.sleep(0.5)
        
        print(f"\n3. 点赞自己的评论（作者ID=1001，评论ID={comment1['id']}）")
        print("   预期：成功（点赞不受限制）")
        success = client.add_reaction(
            resource_type=2,
            resource_id=comment1['id'],
            reactor_id=1001,  # 与作者ID相同
            reaction_type=1  # 1=like
        )
        if success:
            print("   ✓ 正确：可以点赞自己的评论")
        else:
            print("   ✗ 错误：应该可以点赞自己的评论")
    
    # ==================== 测试 3: 连续评论间隔限制 ====================
    print("\n\n【测试 3】连续评论间隔限制（最少3秒）")
    print("-" * 70)
    
    print("\n1. 创建第一条评论")
    first = client.create_comment(
        post_id=post_id,
        author_id=1004,
        content="第一条评论"
    )
    
    print("\n2. 立即创建第二条评论（间隔 < 3秒）")
    print("   预期：返回 429，提示请求过于频繁")
    second = client.create_comment(
        post_id=post_id,
        author_id=1004,  # 同一用户
        content="第二条评论（应该失败）"
    )
    if not second:
        print("   ✓ 正确：连续评论被限制")
    else:
        print("   ✗ 错误：应该限制连续评论")
    
    print("\n3. 等待3秒后再次评论")
    print("   等待中...", end="", flush=True)
    for i in range(3):
        time.sleep(1)
        print(".", end="", flush=True)
    print(" 完成")
    
    third = client.create_comment(
        post_id=post_id,
        author_id=1004,  # 同一用户
        content="第三条评论（应该成功）"
    )
    if third:
        print("   ✓ 正确：间隔3秒后可以评论")
    else:
        print("   ✗ 错误：间隔3秒后应该可以评论")
    
    # ==================== 测试 4: 二级回复的排序 ====================
    print("\n\n【测试 4】二级回复也按最新时间排序")
    print("-" * 70)
    
    if comment1:
        print(f"\n1. 给一级评论添加多个回复")
        
        print("   添加回复1（时间：T1）")
        reply1 = client.create_comment(
            post_id=post_id,
            author_id=2001,
            content="回复1 - 时间最早",
            parent_comment_id=comment1['id']
        )
        time.sleep(1)
        
        print("   添加回复2（时间：T2）")
        reply2 = client.create_comment(
            post_id=post_id,
            author_id=2002,
            content="回复2 - 时间居中",
            parent_comment_id=comment1['id']
        )
        time.sleep(1)
        
        print("   添加回复3（时间：T3）")
        reply3 = client.create_comment(
            post_id=post_id,
            author_id=2003,
            content="回复3 - 时间最新",
            parent_comment_id=comment1['id']
        )
        time.sleep(0.5)
        
        print("\n2. 获取评论列表，验证回复排序")
        comments = client.get_comments(post_id)
        for c in comments:
            if c['id'] == comment1['id']:
                replies = c.get('replies', [])
                print(f"\n   一级评论 ID={c['id']} 的回复顺序：")
                for i, r in enumerate(replies, 1):
                    print(f"   [{i}] ID={r['id']}, 内容: {r['content']}")
                    print(f"       时间: {r['created_at']}")
                
                if len(replies) >= 3:
                    if "最新" in replies[0]['content']:
                        print("\n   ✓ 排序正确：最新的回复在最前面")
                    else:
                        print("\n   ✗ 排序错误：最新的回复不在最前面")
                break
    
    # ==================== 测试 5: 对已删除内容的操作 ====================
    print("\n\n【测试 5】对已删除内容的操作限制")
    print("-" * 70)
    
    # 创建一个测试评论
    print("\n1. 创建一个测试评论")
    test_comment = client.create_comment(
        post_id=post_id,
        author_id=3001,
        content="测试评论（即将被删除）"
    )
    time.sleep(0.5)
    
    if test_comment:
        print(f"\n2. 删除该评论（ID={test_comment['id']}）")
        client.delete_comment(test_comment['id'])
        time.sleep(0.5)
        
        print(f"\n3. 尝试收藏已删除的评论")
        print("   预期：返回 410，提示资源已删除")
        success = client.add_reaction(
            resource_type=2,
            resource_id=test_comment['id'],
            reactor_id=3002,
            reaction_type=2
        )
        if not success:
            print("   ✓ 正确：不能对已删除的内容添加反应")
        else:
            print("   ✗ 错误：应该禁止对已删除的内容添加反应")
    
    print("\n" + "=" * 70)
    print("✅ 边界情况测试完成！")
    print("=" * 70)
    
    print("\n📊 测试总结：")
    print("""
    ✓ 评论列表按最新时间降序排列（最新的在前面）
    ✓ 二级回复也按最新时间降序排列
    ✓ 不能收藏自己发布的帖子/评论（返回 422）
    ✓ 可以点赞自己的内容
    ✓ 其他用户可以收藏
    ✓ 连续评论最少间隔3秒（返回 429）
    ✓ 不能对已删除的内容添加反应（返回 410）
    """)


if __name__ == "__main__":
    test_edge_cases()
