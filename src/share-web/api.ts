import type {
  FileShareListResponse,
  FileShareRootSummary,
  FileShareSearchResult,
  FileShareSearchScope,
  FileShareSession,
} from './types';

export class FileShareApiError extends Error {
  status: number;

  constructor(status: number, message: string) {
    super(message);
    this.name = 'FileShareApiError';
    this.status = status;
  }
}

async function request<T>(
  path: string,
  init?: RequestInit,
  responseType: 'json' | 'text' | 'empty' = 'json',
): Promise<T> {
  const response = await fetch(path, {
    credentials: 'include',
    ...init,
  });

  if (!response.ok) {
    const message = await response.text().catch(() => response.statusText || 'Request failed');
    throw new FileShareApiError(response.status, message || response.statusText || 'Request failed');
  }

  if (responseType === 'empty') {
    return undefined as T;
  }
  if (responseType === 'text') {
    return (await response.text()) as T;
  }
  return (await response.json()) as T;
}

function buildMultipart(
  root: string,
  parent: string,
  files: File[],
): FormData {
  const data = new FormData();
  data.set('root', root);
  data.set('parent', parent);
  for (const file of files) {
    const relativePath = (file as File & { webkitRelativePath?: string }).webkitRelativePath || file.name;
    data.append('file', file, relativePath);
  }
  return data;
}

function buildDownloadPath(root: string, relativePath: string): string {
  const segments = [root, ...relativePath.split('/').filter(Boolean)];
  return segments.map((segment) => encodeURIComponent(segment)).join('/');
}

export const fileShareApi = {
  getSession() {
    return request<FileShareSession>('/api/session');
  },
  login(accountId: string, password: string) {
    return request<FileShareSession>('/api/auth/login', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({
        account_id: accountId,
        password,
      }),
    });
  },
  logout() {
    return request<void>('/api/auth/logout', {
      method: 'POST',
    }, 'empty');
  },
  listRoots() {
    return request<FileShareRootSummary[]>('/api/roots');
  },
  listEntries(root: string, path = '') {
    const query = new URLSearchParams({ root });
    if (path) {
      query.set('path', path);
    }
    return request<FileShareListResponse>(`/api/list?${query.toString()}`);
  },
  search(keyword: string, scope: FileShareSearchScope, root?: string, path?: string) {
    const query = new URLSearchParams({
      keyword,
      scope,
    });
    if (root) {
      query.set('root', root);
    }
    if (path) {
      query.set('path', path);
    }
    return request<FileShareSearchResult[]>(`/api/search?${query.toString()}`);
  },
  uploadFiles(root: string, parent: string, files: File[]) {
    return request<void>('/api/upload/files', {
      method: 'POST',
      body: buildMultipart(root, parent, files),
    }, 'empty');
  },
  uploadDirectory(root: string, parent: string, files: File[]) {
    return request<void>('/api/upload/directory', {
      method: 'POST',
      body: buildMultipart(root, parent, files),
    }, 'empty');
  },
  createDirectory(root: string, parent: string, name: string) {
    return request<void>('/api/entries/directory', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({
        root,
        parent,
        name,
      }),
    }, 'empty');
  },
  createText(root: string, parent: string, name: string, content: string) {
    return request<void>('/api/entries/text', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({
        root,
        parent,
        name,
        content,
      }),
    }, 'empty');
  },
  rename(root: string, path: string, toName: string) {
    return request<void>('/api/entries/rename', {
      method: 'PATCH',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({
        root,
        path,
        to_name: toName,
      }),
    }, 'empty');
  },
  remove(root: string, path: string) {
    return request<void>('/api/entries', {
      method: 'DELETE',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({
        root,
        path,
      }),
    }, 'empty');
  },
  previewUrl(root: string, path: string) {
    const query = new URLSearchParams({
      root,
      path,
    });
    return `/api/preview?${query.toString()}`;
  },
  downloadFileUrl(root: string, path: string) {
    return `/download/file/${buildDownloadPath(root, path)}`;
  },
  downloadArchiveUrl(root: string, path: string) {
    return `/download/zip/${buildDownloadPath(root, path)}`;
  },
};

export function isUnauthorized(error: unknown): boolean {
  return error instanceof FileShareApiError && error.status === 401;
}

export function isForbidden(error: unknown): boolean {
  return error instanceof FileShareApiError && error.status === 403;
}

export function getErrorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}
