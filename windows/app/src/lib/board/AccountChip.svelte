<script lang="ts">
  // 지금 로그인 계정 — 누르면 저장해 둔 계정으로 전환하거나 계정을 추가하는 메뉴(ClaudeBoard.swift AccountChip).
  // 계정 추가는 설정 > 계정에서 로그인 창·코드 칸을 보며 한다(열고 바로 시작). 작업 중인 세션이 있을 때의 전환 확인과 계정 지우기 확인은 백엔드가 띄운다
  import type { Snapshot } from "$lib/types";
  import { beginAddAccount, cancelAddAccount, deleteAccount, setAutoSwitch, switchAccount } from "$lib/backend";
  import { accountLogin, watchAccountLogin } from "$lib/settings/groups/accountLogin.svelte";
  import { openSettings } from "$lib/settings/screen.svelte";
  import Icon from "./Icon.svelte";
  import { hint } from "./hint.svelte";
  import { openMenu, type MenuItem } from "./menu.svelte";

  let { accounts, compact = false }: { accounts: Snapshot["accounts"]; compact?: boolean } = $props();

  const c = $derived(accounts.current);

  // 로그인 주소·안내는 이벤트로만 오므로 설정 화면이 닫혀 있어도 받아 둔다
  $effect(() => watchAccountLogin());

  // 설정 > 계정을 열고 바로 로그인을 시작한다(email 이 있으면 그 계정으로 다시 로그인)
  function addInSettings(email?: string) {
    openSettings("account");
    accountLogin.reset();
    beginAddAccount(email);
  }

  function showMenu(e: MouseEvent) {
    const items: MenuItem[] = [{ label: "Claude 계정 — 고르면 모든 세션이 그 계정으로 이어 갑니다", header: true }];
    const list = [...accounts.saved];
    if (c && !list.some((a) => a.id === c.id)) list.unshift(c);
    for (const a of list) {
      const isCurrent = a.id === c?.id;
      items.push({
        label: [a.email, a.plan].filter(Boolean).join("  ·  ") + (!isCurrent && a.expired ? "  — 다시 로그인 필요" : ""),
        enabled: !accounts.switching,
        checked: isCurrent,
        title: a.org || undefined,
        action: () => {
          if (isCurrent) return;
          if (a.expired) addInSettings(a.email);
          else switchAccount(a.id);
        },
      });
    }
    items.push({ separator: true });
    if (accounts.adding) {
      items.push({ label: "계정 추가 계속 (설정 > 계정)", action: () => openSettings("account") });
      items.push({ label: "계정 추가 취소", action: cancelAddAccount });
    } else {
      items.push({ label: "계정 추가… (설정 > 계정에서 로그인)", action: () => addInSettings() });
    }
    const removable = accounts.saved.filter((a) => a.id !== c?.id);
    if (removable.length) {
      items.push({
        label: "저장한 계정 지우기",
        submenu: removable.map((a) => ({ label: a.email, action: () => deleteAccount(a.id) })),
      });
    }
    items.push({ separator: true });
    items.push({
      label: "한도가 차면 다른 계정으로 자동 전환",
      checked: accounts.autoSwitch,
      title: "5시간·주간 한도가 차거나 세션이 한도에 걸려 멈추면 저장한 다른 계정으로 바꾸고, 멈춘 세션을 이어서 진행합니다",
      action: () => setAutoSwitch(!accounts.autoSwitch),
    });
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    openMenu(items, { x: r.left, y: r.bottom + 4 });
  }
</script>

<button
  class="chip"
  class:compact
  onclick={showMenu}
  use:hint={`로그인 계정 ${c?.email ?? "없음"}${c?.org ? ` · ${c.org}` : ""} — 눌러서 다른 계정으로 전환하거나 계정을 추가합니다` +
    (accounts.autoSwitch ? " · 한도 자동 전환 켜짐" : "")}
>
  <span class="avatar"><Icon name={accounts.adding ? "person-plus" : accounts.switching ? "person-switch" : "person"} size={15.5} /></span>
  {#if !compact}
    <span class="email">{accounts.adding ? "로그인 기다리는 중…" : (c?.email ?? "로그인 정보 없음")}</span>
    {#if !accounts.adding && c?.plan}<span class="plan">{c.plan}</span>{/if}
  {/if}
  {#if accounts.autoSwitch}<span class="dim"><Icon name="swap" size={9} /></span>{/if}
  <span class="dim"><Icon name="chevron-down" size={11} /></span>
</button>

<style>
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    min-width: 0;
    max-width: 100%;
    min-height: 22px;
    padding: 3px 8px 3px 5px;
    border-radius: 999px;
    background: rgb(255 255 255 / 0.05);
    transition: background 0.15s;
  }
  .chip.compact {
    padding: 3px 6px 3px 4px;
    flex: none;
  }
  .chip:hover {
    background: rgb(255 255 255 / 0.12);
  }
  .avatar {
    display: grid;
    place-items: center;
    color: rgb(var(--accent));
  }
  .email {
    min-width: 0;
    font-size: 11.5px;
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .plan {
    flex: none;
    padding: 1px 5px;
    border-radius: 999px;
    font-size: 9.5px;
    font-weight: 700;
    color: var(--fg2);
    background: rgb(255 255 255 / 0.08);
    white-space: nowrap;
  }
  .dim {
    display: grid;
    place-items: center;
    color: var(--fg2);
  }
</style>
