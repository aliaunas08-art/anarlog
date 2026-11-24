import { HomeView } from "./home";

import { StandardContentWrapper } from "~/shared/main";
import type { Tab } from "~/store/zustand/tabs";

export function TabContentEmpty({
  tab: _tab,
}: {
  tab: Extract<Tab, { type: "empty" }>;
}) {
  return (
    <StandardContentWrapper>
      <HomeView />
    </StandardContentWrapper>
  );
}
