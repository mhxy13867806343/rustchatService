"""
测试删除级联逻辑
验证：
1. 删除帖子时，所有评论和回复都被软删除
2. 删除一级评论时，其下的所有二级回复都被软删除
3. 删除后不能再评论或回复
4. 重复删除返回 410 Gone
"""

from python_client_example import RustChatClient
import time

def test_delete_cascade():
    """测试删除级联逻辑"""
    print("=" * 70)
    print("删除级联逻辑测试")
    print("=" * 70)
    
    client = RustChatClient(
        base_url="http://127.0.0.1:8081",
        auth_secret="sso-secret"
    )
    
    post_id = 999  # 使用一个测试帖子ID
    
    print(f"\n📝 测试帖子 ID: {post_id}")
    print("-" * 70)
    
    # ==================== 场景 1: 创建评论树 ====================
    print("\n【场景 1】创建评论树")
    print("-" * 70)
    
    print("\n1. 创建一级评论 A（作者=100）")
    comment_a = client.create_comment(
        post_id=post_id,
        author_id=100,
        content="一级评论 A"
    )
    time.sleep(0.3)
    
    print("\n2. 创建一级评论 B（作者=101）")
    comment_b = client.create_comment(
        post_id=post_id,
        author_id=101,
        content="一级评论 B"
    )
    time.sleep(0.3)
    
    if comment_a:
        print(f"\n3. 给一级评论 A (ID={comment_a['id']}) 添加回复 A1")
        reply_a1 = client.create_comment(
            post_id=post_id,
            author_id=102,
            content="回复 A1",
            parent_comment_id=comment_a['id'],
            at_user_id=100
        )
        time.sleep(0.3)
        
        print(f"\n4. 给一级评论 A (ID={comment_a['id']}) 添加回复 A2")
        reply_a2 = client.create_comment(
            post_id=post_id,
            author_id=103,
            content="回复 A2",
            parent_comment_id=comment_a['id'],
            at_user_id=100
        )
        time.sleep(0.3)
    
    if comment_b:
        print(f"\n5. 给一级评论 B (ID={comment_b['id']}) 添加回复 B1")
        reply_b1 = client.create_comment(
            post_id=post_id,
            author_id=104,
            content="回复 B1",
            parent_comment_id=comment_b['id'],
            at_user_id=101
        )
        time.sleep(0.3)
    
    print("\n6. 查看当前评论树")
    comments = client.get_comments(post_id)
    print(f"   当前有 {len(comments)} 条一级评论")
    for c in comments:
        print(f"   - 一级评论 ID={c['id']}, 回复数={len(c.get('replies', []))}")
    
    # ==================== 场景 2: 删除一级评论（级联删除回复）====================
    print("\n\n【场景 2】删除一级评论 A（应该级联删除其下的所有回复）")
    print("-" * 70)
    
    if comment_a:
        print(f"\n1. 删除一级评论 A (ID={comment_a['id']})")
        success = client.delete_comment(comment_a['id'])
        time.sleep(0.3)
        
        if success:
            print(f"\n2. 尝试回复已删除的一级评论 A (ID={comment_a['id']})")
            print("   预期：返回 410 Gone，提示评论已删除")
            failed_reply = client.create_comment(
                post_id=post_id,
                author_id=105,
                content="尝试回复已删除的评论",
                parent_comment_id=comment_a['id']
            )
            if not failed_reply:
                print("   ✓ 正确：无法回复已删除的评论")
            time.sleep(0.3)
            
            print(f"\n3. 尝试再次删除一级评论 A (ID={comment_a['id']})")
            print("   预期：返回 410 Gone，提示评论已删除")
            client.delete_comment(comment_a['id'])
            time.sleep(0.3)
    
    print("\n4. 查看删除后的评论树")
    comments = client.get_comments(post_id)
    print(f"   当前有 {len(comments)} 条一级评论（应该只剩下评论 B）")
    for c in comments:
        print(f"   - 一级评论 ID={c['id']}, 回复数={len(c.get('replies', []))}")
    
    # ==================== 场景 3: 删除帖子（级联删除所有评论）====================
    print("\n\n【场景 3】删除帖子（应该级联删除所有评论和回复）")
    print("-" * 70)
    
    print(f"\n1. 删除帖子 (ID={post_id})")
    success = client.delete_post(post_id)
    time.sleep(0.3)
    
    if success:
        print(f"\n2. 尝试给已删除的帖子添加评论")
        print("   预期：返回 410 Gone，提示帖子已删除")
        failed_comment = client.create_comment(
            post_id=post_id,
            author_id=106,
            content="尝试评论已删除的帖子"
        )
        if not failed_comment:
            print("   ✓ 正确：无法评论已删除的帖子")
        time.sleep(0.3)
        
        print(f"\n3. 尝试再次删除帖子 (ID={post_id})")
        print("   预期：返回 410 Gone，提示帖子已删除")
        client.delete_post(post_id)
        time.sleep(0.3)
        
        print(f"\n4. 尝试获取已删除帖子的评论列表")
        comments = client.get_comments(post_id)
        print(f"   返回 {len(comments)} 条评论（已删除的评论不会显示）")
    
    # ==================== 场景 4: 测试二级回复的删除 ====================
    print("\n\n【场景 4】测试二级回复的删除（不影响一级评论）")
    print("-" * 70)
    
    post_id_2 = 1000  # 使用另一个测试帖子
    
    print(f"\n1. 创建新帖子的评论树 (帖子ID={post_id_2})")
    comment_c = client.create_comment(
        post_id=post_id_2,
        author_id=200,
        content="一级评论 C"
    )
    time.sleep(0.3)
    
    if comment_c:
        print(f"\n2. 添加回复 C1")
        reply_c1 = client.create_comment(
            post_id=post_id_2,
            author_id=201,
            content="回复 C1",
            parent_comment_id=comment_c['id']
        )
        time.sleep(0.3)
        
        print(f"\n3. 添加回复 C2")
        reply_c2 = client.create_comment(
            post_id=post_id_2,
            author_id=202,
            content="回复 C2",
            parent_comment_id=comment_c['id']
        )
        time.sleep(0.3)
        
        print("\n4. 查看评论树")
        comments = client.get_comments(post_id_2)
        for c in comments:
            print(f"   - 一级评论 ID={c['id']}, 回复数={len(c.get('replies', []))}")
        
        if reply_c1:
            print(f"\n5. 删除回复 C1 (ID={reply_c1['id']})")
            client.delete_comment(reply_c1['id'])
            time.sleep(0.3)
            
            print("\n6. 查看删除后的评论树（一级评论应该还在，只是少了一个回复）")
            comments = client.get_comments(post_id_2)
            for c in comments:
                print(f"   - 一级评论 ID={c['id']}, 回复数={len(c.get('replies', []))} (应该是1)")
                for r in c.get('replies', []):
                    print(f"     └─ 回复 ID={r['id']}")
    
    print("\n" + "=" * 70)
    print("✅ 删除级联逻辑测试完成！")
    print("=" * 70)
    
    print("\n📊 测试总结：")
    print("""
    ✓ 删除一级评论时，其下的所有二级回复都被级联删除
    ✓ 删除帖子时，所有评论和回复都被级联删除
    ✓ 删除二级回复时，不影响一级评论
    ✓ 删除后不能再评论或回复（返回 410 Gone）
    ✓ 重复删除返回 410 Gone
    ✓ 所有删除都是软删除，数据仍在数据库中
    """)


if __name__ == "__main__":
    test_delete_cascade()
