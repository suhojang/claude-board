<script lang="ts">
  // 설정 > 계정 — 저장한 계정(전환·다시 로그인·지우기)·계정 추가 로그인·한도 자동 전환(mac AccountList·AccountLoginPanel).
  // 줄은 SettingsCard 로 그린다. 로그인 진행(주소·안내)은 accountLogin.svelte 가 쥐고, 규칙은 accountLogin.ts 가 쥔다.
  // 지우기 확인은 백엔드가 받는다(deleteAccount). 로그인 창은 백엔드가 띄운다(닫으면 취소).
  import { board } from "$lib/board/store.svelte";
  import { beginAddAccount, cancelAddAccount, deleteAccount, openLoginUrl, setAutoSwitch, submitLoginCode, switchAccount } from "$lib/backend";
  import SettingsCard from "../SettingsCard.svelte";
  import { canSubmitCode, loginSteps } from "./accountLogin";
  import { accountLogin, watchAccountLogin } from "./accountLogin.svelte";

  const accounts = $derived(board.snap?.accounts);
  const current = $derived(accounts?.current ?? null);
  // 지금 계정이 저장 목록에 없어도 맨 위에 보인다
  const list = $derived.by(() => {
    if (!accounts) return [];
    const all = [...accounts.saved];
    if (current && !all.some((a) => a.id === current.id)) all.unshift(current);
    return all;
  });
  const steps = $derived(loginSteps({ url: accountLogin.url, note: accountLogin.note }));

  let code = $state("");
  let copied = $state(false);

  $effect(() => watchAccountLogin());

  function beginAdd(email?: string) {
    accountLogin.reset();
    copied = false;
    beginAddAccount(email);
  }

  function sendCode() {
    if (!canSubmitCode(code)) return;
    submitLoginCode(code);
    code = "";
  }

  async function copyLink() {
    if (!accountLogin.url) return;
    try {
      await navigator.clipboard.writeText(accountLogin.url);
      copied = true;
    } catch {
      copied = false;
    }
  }

  function describe(org: string, plan: string): string | undefined {
    return [org, plan].filter(Boolean).join(" · ") || undefined;
  }
</script>

{#if accounts}
  <div class="group" data-testid="settings-account">
    <h2>Claude 계정</h2>
    <p class="lead">고르면 모든 세션이 다음 요청부터 그 계정으로 이어 갑니다</p>

    {#each list as a (a.id)}
      {@const isCurrent = a.id === current?.id}
      <SettingsCard title={a.email} detail={describe(a.org, a.plan)}>
        {#if isCurrent}
          <span class="inuse" data-testid="account-current">사용 중</span>
        {:else}
          {#if a.expired}
            <button class="btn" data-testid="account-relogin" disabled={accounts.switching || accounts.adding} onclick={() => beginAdd(a.email)}>다시 로그인</button>
          {:else}
            <button class="btn" data-testid="account-switch" disabled={accounts.switching} onclick={() => switchAccount(a.id)}>전환</button>
          {/if}
          <button class="btn quiet" data-testid="account-delete" title="이 계정의 저장된 로그인 정보를 지웁니다" onclick={() => deleteAccount(a.id)}>지우기</button>
        {/if}
      </SettingsCard>
    {:else}
      <p class="empty">로그인 정보가 없습니다</p>
    {/each}

    {#if accounts.adding}
      <div class="login" data-testid="account-login" data-url={accountLogin.url ?? ""}>
        <SettingsCard title="새 계정 로그인">
          <button class="btn" data-testid="login-cancel" onclick={cancelAddAccount}>취소</button>
        </SettingsCard>
        <SettingsCard title="1. 열린 로그인 창에서 추가할 계정으로 로그인하세요" detail={steps.openDetail}>
          {#if steps.hasUrl}
            <button class="btn" data-testid="login-link" onclick={openLoginUrl}>다른 브라우저에서 열기</button>
            <button class="btn quiet" data-testid="login-copy" onclick={copyLink}>{copied ? "복사했습니다" : "링크 복사"}</button>
          {/if}
        </SettingsCard>
        <SettingsCard title="2. 브라우저가 코드를 보여 주면 여기에 붙여넣으세요" detail={'로그인 뒤 "코드를 복사해 Claude Code 에 붙여넣으라"는 화면이 나올 때만 필요합니다'}>
          <input
            class="code"
            data-testid="login-code"
            placeholder="인증 코드"
            autocomplete="off"
            spellcheck="false"
            bind:value={code}
            onkeydown={(e) => e.key === "Enter" && !e.isComposing && sendCode()}
          />
          <button class="btn prominent" data-testid="login-code-submit" disabled={!canSubmitCode(code)} onclick={sendCode}>확인</button>
        </SettingsCard>
        {#if steps.note}
          <p class="note" data-testid="login-note">{steps.note}</p>
        {/if}
      </div>
    {:else}
      <SettingsCard title="계정 추가…" detail="브라우저에서 추가할 계정으로 로그인합니다">
        <button class="btn prominent" data-testid="account-add" disabled={accounts.switching} onclick={() => beginAdd()}>계정 추가…</button>
      </SettingsCard>
    {/if}

    {#if accounts.notice}
      <p class="notice" class:error={accounts.notice.error} data-testid="account-notice">{accounts.notice.text}</p>
    {/if}

    <h2>자동 전환</h2>
    <SettingsCard
      title="한도가 차면 다른 계정으로 자동 전환"
      detail="5시간·주간 한도가 차거나 세션이 한도에 걸려 멈추면 저장한 다른 계정으로 바꾸고, 멈춘 세션을 이어서 진행합니다"
    >
      <input type="checkbox" data-testid="account-auto-switch" checked={accounts.autoSwitch} onchange={(e) => setAutoSwitch(e.currentTarget.checked)} />
    </SettingsCard>
  </div>
{/if}

<style>
  .group {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 16px 20px 24px;
  }
  h2 {
    margin: 10px 0 0;
    font-size: 13px;
    font-weight: 700;
    color: var(--fg);
  }
  .lead,
  .empty {
    margin: 0;
    font-size: 11.5px;
    color: var(--fg2);
  }
  .login {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 8px;
    border-radius: 10px;
    background: rgb(var(--accent) / 0.07);
    box-shadow: inset 0 0 0 1px rgb(var(--accent) / 0.25);
  }
  .note {
    margin: 0;
    font-size: 11.5px;
    color: var(--fg2);
  }
  .notice {
    margin: 0;
    font-size: 11.5px;
    color: var(--fg2);
  }
  .notice.error {
    color: rgb(var(--orange));
  }
  .inuse {
    padding: 2px 8px;
    border-radius: 999px;
    font-size: 11px;
    font-weight: 600;
    color: rgb(var(--idle));
    background: rgb(var(--idle) / 0.12);
  }
  .btn {
    padding: 4px 10px;
    border-radius: 7px;
    font-size: 12px;
    font-weight: 600;
    background: rgb(255 255 255 / 0.08);
  }
  .btn:hover:not(:disabled) {
    background: rgb(255 255 255 / 0.14);
  }
  .btn:disabled {
    opacity: 0.4;
  }
  .btn.quiet {
    color: var(--fg2);
    background: transparent;
  }
  .btn.prominent {
    background: rgb(var(--accent) / 0.85);
    color: #fff;
  }
  .code {
    min-width: 0;
    width: 220px;
    padding: 5px 9px;
    border-radius: 7px;
    font-family: var(--mono);
    font-size: 12px;
    color: var(--fg);
    background: rgb(0 0 0 / 0.25);
    box-shadow: inset 0 0 0 1px rgb(255 255 255 / 0.1);
  }
  .code:focus {
    outline: none;
    box-shadow: inset 0 0 0 1px rgb(var(--accent) / 0.7);
  }
</style>
