import { AuthForm } from "@/components/web/auth-form";
import { WebShell } from "@/components/web/web-shell";

export default function LoginPage() {
  return (
    <WebShell
      activePath="/login"
      eyebrow="newapi login"
      title="先把登录页独立出来"
      description="这里先按 Web 页面拆出独立登录路由，后面接 newapi 时只改接口层，不再回到桌面主页面里揉状态。"
    >
      <AuthForm mode="login" />
    </WebShell>
  );
}
