export type DashboardStat = {
  title: string;
  value: string;
  unit: string;
  detail: string;
  tone?: "brass" | "ink";
};

export type DashboardStatsPayload = {
  total_captures: number;
  effective_captures: number;
  skipped_captures: number;
  vlm_processed: number;
  scheduler_enabled: boolean;
  is_recording: boolean;
  server_running: boolean;
  batch_running: boolean;
  next_batch_run_at: string | null;
  last_capture_at: string | null;
  last_error: string | null;
};

export type VlmProgressPayload = {
  total: number;
  completed: number;
  failed: number;
  current_id: string | null;
  estimated_remaining_secs: number | null;
};

export type RecentCapture = {
  id: string;
  timestamp: string;
  app: string;
  window_title: string;
  image_path: string | null;
  description: string | null;
  dhash: string | null;
  vlm_processed: boolean;
};

export type DashboardSnapshot = {
  stats: DashboardStatsPayload;
  vlm_progress: VlmProgressPayload;
  recent_captures: RecentCapture[];
};
