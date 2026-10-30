import { listen } from "@tauri-apps/api/event";
import "./style.css";

type AppStat = {
  id: string;
  name: string;
  path: string;
  down_bps: number;
  up_bps: number;
  down_total: number;
  up_total: number;
  limit: { down_bps: number | null; up_bps: number | null } | null;
  blocked: boolean;
};

const status = document.querySelector<HTMLParagraphElement>("#status");

void listen<AppStat[]>("stats", ({ payload }) => {
  if (status) status.textContent = `${payload.length} applications reporting`;
});
