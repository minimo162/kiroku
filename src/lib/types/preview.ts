export type PreviewCaptureRecord = {
  id: string;
  timestamp: string;
  app: string;
  window_title: string;
  image_path: string | null;
  image_exists: boolean;
  description: string | null;
  dhash: string | null;
  vlm_processed: boolean;
};

export type CaptureDateGroup = {
  date: string;
  count: number;
};

export type DescriptionHistoryRecord = {
  capture_id: string;
  previous_description: string | null;
  new_description: string | null;
  edited_at: string;
};

export type PreviewPagePayload = {
  selected_date: string | null;
  available_dates: CaptureDateGroup[];
  total: number;
  page: number;
  page_size: number;
  records: PreviewCaptureRecord[];
};
