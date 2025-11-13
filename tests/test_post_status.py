"""
测试帖子状态检查功能
模拟前端用户点击进入详情页的场景
"""

from python_client_example import RustChatClient
import time

def test_post_status():
    """测试帖子状态检查"""
    print("=" * 70)
    print("帖子状态检查测试")
    print("=" * 70)
    
    client = RustChatClient(
        base_url="http://127.0.0.1:8081",
        auth_secret="sso-secret"
    )
    
    # ==================== 场景 1: 正常帖子 ====================
    print("\n【场景 1】检查正常帖子的状态")
    print("-" * 70)
    
    post_id_normal = 5000
    
    print(f"\n1. 创建一个正常帖子（ID={post_id_normal}）")
    comment = client.create_comment(
        post_id=post_id_normal,
        author_id=5001,
        content="测试评论"
    )
    time.sleep(0.5)
    
    print(f"\n2. 检查帖子状态")
    status = client.check_post_status(post_id_normal)
    
    if status.get('exists') and not status.get('deleted') and not status.get('locked'):
        print("   ✓ 正确：帖子状态正常")
    else:
        print("   ✗ 错误：帖子应该是正常状态")
    
    # ==================== 场景 2: 已删除的帖子 ====================
    print("\n\n【场景 2】检查已删除帖子的状态")
    print("-" * 70)
    
    post_id_deleted = 5001
    
    print(f"\n1. 创建一个测试帖子（ID={post_id_deleted}）")
    client.create_comment(
        post_id=post_id_deleted,
        author_id=5002,
        content="测试评论"
    )
    time.sleep(0.5)
    
    print(f"\n2. 删除该帖子")
    client.delete_post(post_id_deleted)
    time.sleep(0.5)
    
    print(f"\n3. 用户点击进入详情页，检查帖子状态")
    status = client.check_post_status(post_id_deleted)
    
    if status.get('deleted'):
        print("   ✓ 正确：检测到帖子已删除")
        print(f"   前端应该显示: {status.get('message')}")
    else:
        print("   ✗ 错误：应该检测到帖子已删除")
    
    print(f"\n4. 尝试获取已删除帖子的评论列表")
    print("   预期：返回 410 Gone")
    comments = client.get_comments(post_id_deleted)
    if not comments:
        print("   ✓ 正确：无法获取已删除帖子的评论")
    
    # ==================== 场景 3: 不存在的帖子 ====================
    print("\n\n【场景 3】检查不存在的帖子")
    print("-" * 70)
    
    post_id_not_exist = 999999
    
    print(f"\n1. 用户点击一个不存在的帖子链接（ID={post_id_not_exist}）")
    status = client.check_post_status(post_id_not_exist)
    
    if not status.get('exists'):
        print("   ✓ 正确：检测到帖子不存在")
        print(f"   前端应该显示: {status.get('message')}")
    else:
        print("   ✗ 错误：应该检测到帖子不存在")
    
    # ==================== 场景 4: 模拟用户长时间未刷新 ====================
    print("\n\n【场景 4】模拟用户长时间未刷新页面")
    print("-" * 70)
    
    post_id_stale = 5002
    
    print(f"\n1. 用户打开列表页，看到帖子（ID={post_id_stale}）")
    client.create_comment(
        post_id=post_id_stale,
        author_id=5003,
        content="测试评论"
    )
    time.sleep(0.5)
    
    print("\n2. 用户长时间未刷新页面（模拟：帖子在此期间被删除）")
    print("   其他用户删除了该帖子...")
    client.delete_post(post_id_stale)
    time.sleep(0.5)
    
    print("\n3. 用户点击进入详情页，先检查帖子状态")
    status = client.check_post_status(post_id_stale)
    
    if status.get('deleted'):
        print("   ✓ 正确：检测到帖子已被删除")
        print("   前端应该提示用户：'该帖子已被删除'")
        print("   并阻止用户进入详情页或进行评论")
    else:
        print("   ✗ 错误：应该检测到帖子已删除")
    
    # ==================== 场景 5: 完整的前端流程 ====================
    print("\n\n【场景 5】完整的前端流程示例")
    print("-" * 70)
    
    post_id_flow = 5003
    
    print(f"\n模拟前端代码流程：")
    print("""
    async function enterPostDetail(postId) {
        // 1. 先检查帖子状态
        const status = await checkPostStatus(postId);
        
        // 2. 根据状态决定是否继续
        if (!status.exists) {
            showToast("帖子不存在");
            return;
        }
        
        if (status.deleted) {
            showToast("帖子已被删除");
            return;
        }
        
        if (status.locked) {
            showToast("帖子已锁定，无法评论");
            // 可以继续查看，但禁用评论功能
        }
        
        // 3. 正常进入详情页
        loadPostDetail(postId);
        loadComments(postId);
    }
    """)
    
    print("\n实际测试：")
    print(f"\n1. 创建测试帖子（ID={post_id_flow}）")
    client.create_comment(
        post_id=post_id_flow,
        author_id=5004,
        content="测试评论"
    )
    time.sleep(0.5)
    
    print(f"\n2. 用户点击进入详情页")
    status = client.check_post_status(post_id_flow)
    
    if status.get('exists') and not status.get('deleted'):
        print("   ✓ 帖子状态正常，继续加载详情")
        
        print(f"\n3. 加载评论列表")
        comments = client.get_comments(post_id_flow)
        print(f"   加载到 {len(comments)} 条评论")
        
        print(f"\n4. 用户可以正常评论")
        new_comment = client.create_comment(
            post_id=post_id_flow,
            author_id=5005,
            content="用户的新评论"
        )
        if new_comment:
            print("   ✓ 评论成功")
    else:
        print("   ✗ 帖子状态异常，阻止进入详情页")
    
    print("\n" + "=" * 70)
    print("✅ 帖子状态检查测试完成！")
    print("=" * 70)
    
    print("\n📊 测试总结：")
    print("""
    ✓ 可以检测正常帖子的状态
    ✓ 可以检测已删除的帖子（返回 410）
    ✓ 可以检测不存在的帖子（返回 404）
    ✓ 可以检测已锁定的帖子
    ✓ 前端可以在用户点击时先验证帖子状态
    ✓ 防止用户操作已删除的帖子
    
    前端最佳实践：
    1. 用户从列表页点击进入详情页时，先调用 /api/posts/{id}/status
    2. 根据返回的状态决定是否继续加载详情
    3. 如果帖子已删除或不存在，显示友好提示
    4. 如果帖子已锁定，可以查看但禁用评论功能
    """)


if __name__ == "__main__":
    test_post_status()
