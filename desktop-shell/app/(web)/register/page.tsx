import { AuthForm } from "@/components/web/auth-form";
import { WebShell } from "@/components/web/web-shell";

export default function RegisterPage() {
  return (
    <WebShell
      activePath="/register"
      eyebrow="newapi register"
      title="注册页先独立成路由"
      description="先把注册输入、结果提示和跳转链路梳顺，后续无论 newapi 实际是手机号、邮箱还是用户名注册，都只需要替换表单字段和请求映射。"
    >
      <AuthForm mode="register" />
    </WebShell>
  );
}
