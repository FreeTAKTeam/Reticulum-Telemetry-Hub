import { createPinia, setActivePinia } from "pinia";
import { createApp, nextTick } from "vue";
import { createMemoryHistory, createRouter } from "vue-router";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import UsersPage from "../src/pages/UsersPage.vue";
import { useToastStore } from "../src/stores/toasts";

const { delMock, getMock, postMock, putMock } = vi.hoisted(() => ({
  delMock: vi.fn(),
  getMock: vi.fn(),
  postMock: vi.fn(),
  putMock: vi.fn()
}));

vi.mock("../src/api/client", () => ({
  del: delMock,
  get: getMock,
  post: postMock,
  put: putMock
}));

type MountedPage = {
  root: HTMLElement;
  unmount: () => void;
};

const clientAlpha = {
  identity: "client-alpha",
  display_name: "Alpha Client",
  last_seen: "2026-06-23T06:00:00Z",
  metadata: {
    display_name: "Alpha Client",
    announce: {
      destination_hash: "client-alpha",
      source_interface: "delivery_app_data"
    }
  },
  client_type: "generic_lxmf",
  is_rem_capable: false
};

const clientRem = {
  identity: "client-rem",
  display_name: "Rescue Mobile",
  last_seen: "2026-06-23T06:02:00Z",
  metadata: {},
  client_type: "rem",
  announce_capabilities: ["r3akt", "emergencymessages"],
  is_rem_capable: true
};

const identityAlpha = {
  Identity: "client-alpha",
  DisplayName: "Alpha Client",
  Status: "active",
  LastSeen: "2026-06-23T06:00:00Z",
  Metadata: {},
  IsBanned: false,
  IsBlackholed: false,
  ClientType: "generic_lxmf",
  IsRemCapable: false
};

const identityBravo = {
  Identity: "identity-bravo",
  DisplayName: "Bravo Identity",
  Status: "active",
  LastSeen: "2026-06-23T06:01:00Z",
  Metadata: {},
  IsBanned: false,
  IsBlackholed: false,
  ClientType: "generic_lxmf",
  IsRemCapable: true
};

const remPeerAlpha = {
  identity: "rem-alpha",
  destination_hash: "rem-destination-alpha",
  display_name: "REM Alpha",
  announce_capabilities: ["r3akt", "checklist"],
  client_type: "rem",
  registered_mode: "connected",
  status: "active",
  last_seen: "2026-06-23T06:02:00Z"
};

const routingPayload = {
  destinations: [
    {
      destination_hash: "client-alpha",
      identity: "client-alpha",
      display_name: "Alpha Client"
    },
    {
      destination_hash: "route-bravo",
      identity: "identity-bravo",
      display_name: "Bravo Route"
    }
  ]
};

const text = () => document.body.textContent ?? "";

const waitForUi = async () => {
  await nextTick();
  await new Promise((resolve) => window.setTimeout(resolve, 0));
  await nextTick();
};

const clickButton = async (label: string, index = 0) => {
  const matches = Array.from(document.querySelectorAll("button")).filter((button) =>
    (button.textContent ?? "").includes(label)
  );
  const button = matches[index];
  if (!(button instanceof HTMLButtonElement)) {
    throw new Error(`Button not found: ${label}`);
  }
  button.click();
  await waitForUi();
};

const clickCardButton = async (cardText: string, label: string) => {
  const card = Array.from(document.querySelectorAll(".registry-card")).find((entry) =>
    (entry.textContent ?? "").includes(cardText)
  );
  if (!(card instanceof HTMLElement)) {
    throw new Error(`Card not found: ${cardText}`);
  }
  const button = Array.from(card.querySelectorAll("button")).find((entry) =>
    (entry.textContent ?? "").includes(label)
  );
  if (!(button instanceof HTMLButtonElement)) {
    throw new Error(`Button not found in ${cardText}: ${label}`);
  }
  button.click();
  await waitForUi();
};

const clickClientTypeFilter = async (groupLabel: string, label: string) => {
  const group = document.querySelector(`[aria-label="${groupLabel}"]`);
  const button = Array.from(group?.querySelectorAll("button") ?? []).find(
    (entry) => (entry.textContent ?? "").trim() === label
  );
  if (!(button instanceof HTMLButtonElement)) {
    throw new Error(`Client type filter not found: ${groupLabel} / ${label}`);
  }
  button.click();
  await waitForUi();
};

const mountPage = async (): Promise<MountedPage> => {
  const root = document.createElement("div");
  document.body.appendChild(root);
  const pinia = createPinia();
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: "/users", component: UsersPage }]
  });
  await router.push("/users");
  await router.isReady();
  setActivePinia(pinia);
  const app = createApp(UsersPage);
  app.use(pinia);
  app.use(router);
  app.mount(root);
  await waitForUi();
  return {
    root,
    unmount: () => {
      app.unmount();
      root.remove();
    }
  };
};

const currentIdentities = () => [identityAlpha, identityBravo];

const setupApiMocks = () => {
  getMock.mockImplementation((path: string) => {
    if (path === "/Client") {
      return Promise.resolve([clientAlpha, clientRem]);
    }
    if (path === "/Identities") {
      return Promise.resolve(currentIdentities());
    }
    if (path === "/api/rem/peers") {
      return Promise.resolve({ effective_connected_mode: true, items: [remPeerAlpha] });
    }
    if (path === "/Command/DumpRouting") {
      return Promise.resolve(routingPayload);
    }
    if (path === "/api/r3akt/missions" || path === "/api/r3akt/teams" || path === "/api/r3akt/team-members") {
      return Promise.resolve([]);
    }
    return Promise.reject(new Error(`Unexpected GET ${path}`));
  });
  postMock.mockImplementation((path: string) => {
    if (path === "/Client/client-alpha/Ban") {
      return Promise.resolve({ ...identityAlpha, IsBanned: true });
    }
    if (path === "/Client/client-alpha/Unban") {
      return Promise.resolve({ ...identityAlpha, IsBanned: false, IsBlackholed: false });
    }
    if (path === "/Client/client-alpha/Blackhole") {
      return Promise.resolve({ ...identityAlpha, IsBanned: true, IsBlackholed: true });
    }
    if (path === "/Client/identity-bravo/Ban") {
      return Promise.resolve({ ...identityBravo, IsBanned: true });
    }
    if (path === "/Client/identity-bravo/Unban") {
      return Promise.resolve({ ...identityBravo, IsBanned: false, IsBlackholed: false });
    }
    if (path === "/Client/identity-bravo/Blackhole") {
      return Promise.resolve({ ...identityBravo, IsBlackholed: true });
    }
    if (path === "/RTH?identity=identity-bravo" || path === "/RTH?identity=rem-alpha") {
      return Promise.resolve(true);
    }
    return Promise.reject(new Error(`Unexpected POST ${path}`));
  });
  putMock.mockImplementation((path: string) => {
    if (path === "/RTH?identity=client-alpha") {
      return Promise.resolve(true);
    }
    return Promise.reject(new Error(`Unexpected PUT ${path}`));
  });
  delMock.mockResolvedValue(undefined);
};

describe("users page", () => {
  let page: MountedPage | null = null;

  beforeEach(() => {
    document.body.innerHTML = "";
    delMock.mockReset();
    getMock.mockReset();
    postMock.mockReset();
    putMock.mockReset();
    setupApiMocks();
    vi.useFakeTimers({ shouldAdvanceTime: true });
  });

  afterEach(() => {
    page?.unmount();
    page = null;
    vi.useRealTimers();
  });

  it("drives client, identity, REM peer, and routing actions from visible controls", async () => {
    page = await mountPage();
    const toastStore = useToastStore();

    expect(text()).toContain("Alpha Client");
    expect(text()).toContain("Generic LXMF");
    expect(text()).toContain("announce:destination_hash=client-alpha");
    expect(text()).not.toContain("[object Object]");

    await clickCardButton("Alpha Client", "Ban");

    expect(postMock).toHaveBeenCalledWith("/Client/client-alpha/Ban");
    expect(toastStore.messages.at(-1)?.message).toBe("Client ban action sent");

    await clickCardButton("Alpha Client", "Blackhole");

    expect(postMock).toHaveBeenCalledWith("/Client/client-alpha/Blackhole");
    expect(toastStore.messages.at(-1)?.message).toBe("Client blackhole action sent");

    await clickCardButton("Alpha Client", "Unban");

    expect(postMock).toHaveBeenCalledWith("/Client/client-alpha/Unban");
    expect(toastStore.messages.at(-1)?.message).toBe("Client unban action sent");

    await clickCardButton("Alpha Client", "Leave");

    expect(putMock).toHaveBeenCalledWith("/RTH?identity=client-alpha");
    expect(toastStore.messages.at(-1)?.message).toBe("Client left the hub");

    await clickButton("Identities", 1);

    expect(text()).toContain("Bravo Identity");

    await clickCardButton("Bravo Identity", "Ban");

    expect(postMock).toHaveBeenCalledWith("/Client/identity-bravo/Ban");
    expect(toastStore.messages.at(-1)?.message).toBe("Identity ban action sent");
    expect(text()).toContain("Banned");

    await clickCardButton("Bravo Identity", "Join");

    expect(postMock).toHaveBeenCalledWith("/RTH?identity=identity-bravo");
    expect(toastStore.messages.at(-1)?.message).toBe("Identity joined");

    await clickButton("REM Peers", 1);

    expect(text()).toContain("REM Alpha");
    expect(text()).toContain("Connected Mode Enabled");

    await clickCardButton("REM Alpha", "Add User");

    expect(postMock).toHaveBeenCalledWith("/RTH?identity=rem-alpha");
    expect(toastStore.messages.at(-1)?.message).toBe("Identity joined");

    await clickButton("Routing", 1);

    expect(getMock).toHaveBeenCalledWith("/Command/DumpRouting");
    expect(text()).toContain("Bravo Route");
    expect(text()).toContain("route-bravo");
    expect(text()).toContain("Joined");
  });

  it("filters users and identity announces by client type", async () => {
    page = await mountPage();

    expect(text()).toContain("Alpha Client");
    expect(text()).toContain("Rescue Mobile");

    await clickClientTypeFilter("Filter users by client type", "REM");

    expect(text()).not.toContain("Alpha Client");
    expect(text()).toContain("Rescue Mobile");

    await clickButton("Identities", 1);
    await clickClientTypeFilter("Filter announces by client type", "REM");

    expect(text()).not.toContain("Alpha Client");
    expect(text()).toContain("Bravo Identity");

    await clickClientTypeFilter("Filter announces by client type", "LXMF");

    expect(text()).toContain("Alpha Client");
    expect(text()).not.toContain("Bravo Identity");
  });
});
