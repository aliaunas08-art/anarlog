import {
  RETURN_ORIGIN_TAB_TYPES,
  uniqueIdfromTab,
  useTabs,
} from "~/store/zustand/tabs";

export function leaveAllOverlayTabs() {
  for (let attempt = 0; attempt < 10; attempt++) {
    const { currentTab } = useTabs.getState();
    if (!currentTab) return;
    if (!RETURN_ORIGIN_TAB_TYPES.includes(currentTab.type)) return;
    const { slotId } = currentTab;
    leaveOverlayTab();
    if (useTabs.getState().currentTab?.slotId === slotId) return;
  }
}

/** Show Home (the "empty" tab): reuse an open one, else open a new one. */
export function openHome() {
  const { tabs, currentTab, select, openNew } = useTabs.getState();
  if (currentTab?.type === "empty") return;
  const existingHomeTab = tabs.find((tab) => tab.type === "empty");
  if (existingHomeTab) {
    select(existingHomeTab);
    return;
  }
  openNew({ type: "empty" });
}

export function leaveOverlayTab() {
  const { tabs, currentTab, openCurrent, select, goBack, canGoBack } =
    useTabs.getState();

  if (currentTab?.type === "onboarding" || currentTab?.type === "empty") {
    return;
  }

  const returnToSlotId = currentTab?.returnToSlotId;
  const returnToTab = returnToSlotId
    ? tabs.find(
        (tab) =>
          tab.slotId === returnToSlotId &&
          tab.slotId !== currentTab.slotId &&
          (!currentTab.returnToTabId ||
            uniqueIdfromTab(tab) === currentTab.returnToTabId),
      )
    : null;
  // Two overlays that each return to the other (Projects opened from
  // Settings, Settings opened from Projects) would bounce forever: go Home.
  const returnsHere =
    returnToTab?.returnToSlotId !== undefined &&
    returnToTab.returnToSlotId === currentTab?.slotId;
  if (returnToTab && !returnsHere) {
    select(returnToTab);
    return;
  }

  if (returnToSlotId === currentTab?.slotId && canGoBack) {
    goBack();
    return;
  }

  const existingHomeTab = tabs.find((tab) => tab.type === "empty");
  if (existingHomeTab) {
    select(existingHomeTab);
    return;
  }

  openCurrent({ type: "empty" });
}
