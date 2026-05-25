import {
  downloadDownloadFileId,
  downloadDownloadSecureFileId,
  uploadUpload,
  uploadUploadProfilePicture,
} from './generated';
import type { BinaryFileResponse, UploadResponse } from './generated';

const BASE_URL = 'http://localhost:3000/api/documents';

type ApiResponse<T> = {
  data?: T;
  error?: unknown;
};

function requireData<T>(operation: string, response: ApiResponse<T>): T {
  if (response.error) {
    throw new Error(`${operation} failed: ${JSON.stringify(response.error)}`);
  }

  if (response.data === undefined) {
    throw new Error(`${operation} returned no data`);
  }

  return response.data;
}

export async function uploadFile(
  file: Blob | File,
  baseUrl = BASE_URL,
): Promise<UploadResponse> {
  const response = await uploadUpload({
    baseUrl,
    body: { file },
  });

  return requireData('POST /upload', response);
}

export async function uploadProfilePicture(
  file: Blob | File,
  bearerToken: string,
  baseUrl = BASE_URL,
): Promise<UploadResponse> {
  const response = await uploadUploadProfilePicture({
    baseUrl,
    headers: {
      Authorization: `Bearer ${bearerToken}`,
    },
    body: { file },
  });

  return requireData('POST /upload_profile_picture', response);
}

export async function downloadFile(
  fileId: string,
  baseUrl = BASE_URL,
): Promise<BinaryFileResponse> {
  const response = await downloadDownloadFileId({
    baseUrl,
    path: { file_id: fileId },
  });

  return requireData('GET /download/{file_id}', response);
}

export async function downloadSecureFile(
  fileId: string,
  bearerToken: string,
  baseUrl = BASE_URL,
): Promise<BinaryFileResponse> {
  const response = await downloadDownloadSecureFileId({
    baseUrl,
    headers: {
      Authorization: `Bearer ${bearerToken}`,
    },
    path: { file_id: fileId },
  });

  return requireData('GET /download_secure/{file_id}', response);
}
