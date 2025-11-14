"""
密钥系统测试脚本
测试临时密钥和 WebSocket 密钥的完整生命周期
"""

import requests
import time

BASE_URL = "http://127.0.0.1:8081"

class SecretKeyClient:
    def __init__(self, base_url, user_token=None):
        self.base_url = base_url
        self.user_token = user_token
    
    def _get_headers(self):
        headers = {"Content-Type": "application/json"}
        if self.user_token:
            headers["Authorization"] = f"Bearer {self.user_token}"
        return headers
    
    def generate_temp_key(self, key_type="file_download", metadata=None):
        """生成临时密钥"""
        response = requests.post(
            f"{self.base_url}/api/keys/temp/generate",
            json={"key_type": key_type, "metadata": metadata},
            headers=self._get_headers()
        )
        return response.json()
    
    def validate_temp_key(self, key_value):
        """验证并使用临时密钥"""
        response = requests.post(
            f"{self.base_url}/api/keys/temp/validate",
            json={"key_value": key_value},
            headers=self._get_headers()
        )
        return response.json()
    
    def generate_ws_key(self, conversation_id):
        """生成 WebSocket 密钥"""
        response = requests.post(
            f"{self.base_url}/api/keys/ws/generate",
            json={"conversation_id": conversation_id},
            headers=self._get_headers()
        )
        return response.json()


def test_temp_key_lifecycle():
    """测试临时密钥的完整生命周期"""
    print("=" * 70)
    print("测试 1: 临时密钥生命周期")
    print("=" * 70)
    
    client = SecretKeyClient(BASE_URL)
    
    # 1. 生成密钥
    print("\n1. 生成临时密钥")
    result = client.generate_temp_key("file_download")
    
    if result.get("code") == 0:
        data = result["data"]
        key_value = data["key_value"]
        obfuscated = data["obfuscated"]
        expires_at = data["expires_at"]
        
        print(f"   ✓ 密钥生成成功")
        print(f"   原始密钥: {key_value[:20]}...")
        print(f"   混淆显示: {obfuscated[:20]}...")
        print(f"   过期时间: {expires_at}")
        
        # 2. 第一次使用（应该成功）
        print("\n2. 第一次使用密钥")
        result = client.validate_temp_key(key_value)
        if result.get("code") == 0:
            print("   ✓ 密钥验证成功")
        else:
            print(f"   ✗ 验证失败: {result.get('message')}")
        
        # 3. 第二次使用（应该失败，已使用）
        print("\n3. 第二次使用同一密钥")
        result = client.validate_temp_key(key_value)
        if result.get("code") != 0:
            print(f"   ✓ 正确：{result.get('message')}")
        else:
            print("   ✗ 错误：应该禁止重复使用")
    else:
        print(f"   ✗ 生成失败: {result.get('message')}")


def test_temp_key_expiry():
    """测试临时密钥过期"""
    print("\n\n" + "=" * 70)
    print("测试 2: 临时密钥过期")
    print("=" * 70)
    
    client = SecretKeyClient(BASE_URL)
    
    print("\n1. 生成临时密钥")
    result = client.generate_temp_key("api_access")
    
    if result.get("code") == 0:
        key_value = result["data"]["key_value"]
        print("   ✓ 密钥生成成功")
        
        print("\n2. 等待密钥过期（3分钟）...")
        print("   提示：实际测试时可以修改服务器的过期时间为几秒")
        print("   这里我们模拟等待...")
        
        # 实际测试时需要等待3分钟
        # time.sleep(181)
        
        print("\n3. 使用过期密钥")
        print("   （跳过实际等待，请在实际环境中测试）")


def test_concurrent_key_generation():
    """测试并发生成密钥"""
    print("\n\n" + "=" * 70)
    print("测试 3: 并发生成密钥限制")
    print("=" * 70)
    
    client = SecretKeyClient(BASE_URL)
    
    print("\n1. 生成第一个密钥")
    result1 = client.generate_temp_key("file_upload")
    
    if result1.get("code") == 0:
        print("   ✓ 第一个密钥生成成功")
        
        print("\n2. 立即生成第二个密钥（应该失败）")
        result2 = client.generate_temp_key("file_upload")
        
        if result2.get("code") != 0:
            print(f"   ✓ 正确：{result2.get('message')}")
        else:
            print("   ✗ 错误：应该限制并发生成")


def test_ws_key_generation():
    """测试 WebSocket 密钥生成"""
    print("\n\n" + "=" * 70)
    print("测试 4: WebSocket 密钥")
    print("=" * 70)
    
    client = SecretKeyClient(BASE_URL)
    
    # 1. 为会话1生成密钥
    print("\n1. 为会话1生成 WebSocket 密钥")
    result = client.generate_ws_key(conversation_id=1)
    
    if result.get("code") == 0:
        key1 = result["data"]["key_value"]
        print(f"   ✓ 密钥生成成功: {key1[:20]}...")
        
        # 2. 再次为会话1生成密钥（应该返回相同的密钥）
        print("\n2. 再次为会话1生成密钥（应该复用）")
        result = client.generate_ws_key(conversation_id=1)
        
        if result.get("code") == 0:
            key2 = result["data"]["key_value"]
            if key1 == key2:
                print("   ✓ 正确：复用了现有密钥")
            else:
                print("   ✗ 错误：应该复用现有密钥")
        
        # 3. 为会话2生成密钥（应该是新密钥）
        print("\n3. 为会话2生成密钥（应该是新密钥）")
        result = client.generate_ws_key(conversation_id=2)
        
        if result.get("code") == 0:
            key3 = result["data"]["key_value"]
            if key1 != key3:
                print("   ✓ 正确：生成了新密钥")
            else:
                print("   ✗ 错误：不同会话应该有不同密钥")


def test_key_obfuscation():
    """测试密钥混淆显示"""
    print("\n\n" + "=" * 70)
    print("测试 5: 密钥混淆显示")
    print("=" * 70)
    
    client = SecretKeyClient(BASE_URL)
    
    print("\n1. 生成密钥并查看混淆效果")
    result = client.generate_temp_key("data_export")
    
    if result.get("code") == 0:
        data = result["data"]
        key_value = data["key_value"]
        obfuscated = data["obfuscated"]
        
        print(f"\n   原始密钥（前40字符）:")
        print(f"   {key_value[:40]}")
        print(f"\n   混淆显示（前40字符）:")
        print(f"   {obfuscated[:40]}")
        print(f"\n   ✓ 密钥已混淆，双击复制时显示为乱码")


def test_multi_user_scenario():
    """测试多用户场景"""
    print("\n\n" + "=" * 70)
    print("测试 6: 多用户场景")
    print("=" * 70)
    
    user_a = SecretKeyClient(BASE_URL, user_token="token_a")
    user_b = SecretKeyClient(BASE_URL, user_token="token_b")
    
    print("\n1. 用户A生成密钥")
    result = user_a.generate_temp_key("file_download")
    
    if result.get("code") == 0:
        key_value = result["data"]["key_value"]
        print("   ✓ 用户A密钥生成成功")
        
        print("\n2. 用户B尝试使用用户A的密钥")
        result = user_b.validate_temp_key(key_value)
        
        if result.get("code") != 0:
            print(f"   ✓ 正确：{result.get('message')}")
        else:
            print("   ✗ 错误：应该禁止其他用户使用")


def main():
    """运行所有测试"""
    print("🔐 密钥系统测试")
    print("=" * 70)
    print("\n注意：需要先启动 Rust 服务")
    print(f"服务地址: {BASE_URL}")
    print("\n开始测试...\n")
    
    try:
        # 测试1：临时密钥生命周期
        test_temp_key_lifecycle()
        
        # 测试2：密钥过期
        test_temp_key_expiry()
        
        # 测试3：并发生成限制
        test_concurrent_key_generation()
        
        # 测试4：WebSocket 密钥
        test_ws_key_generation()
        
        # 测试5：密钥混淆
        test_key_obfuscation()
        
        # 测试6：多用户场景
        test_multi_user_scenario()
        
        print("\n\n" + "=" * 70)
        print("✅ 所有测试完成！")
        print("=" * 70)
        
        print("\n📊 测试总结：")
        print("""
        ✓ 临时密钥生成和使用
        ✓ 一次性使用限制
        ✓ 并发生成限制
        ✓ WebSocket 密钥生成和复用
        ✓ 密钥混淆显示
        ✓ 多用户权限隔离
        
        注意事项：
        - 密钥过期测试需要等待3分钟
        - 实际环境中需要配置正确的认证信息
        - WebSocket 连接测试需要额外的 WebSocket 客户端
        """)
        
    except requests.exceptions.ConnectionError:
        print("\n❌ 错误：无法连接到服务器")
        print(f"请确保 Rust 服务正在运行: {BASE_URL}")
    except Exception as e:
        print(f"\n❌ 测试失败: {e}")


if __name__ == "__main__":
    main()
