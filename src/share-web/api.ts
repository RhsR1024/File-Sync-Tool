import type {
  FileShareResolveResponse,
  FileShareSearchResponse,
  FileShareSession,
  FileShareTreeResponse,
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

function buildMultipart(parentNodeId: string, files: File[]): FormData {
  const data = new FormData();
  data.set('parent_node_id', parentNodeId);
  for (const file of files) {
    const relativePath = (file as File & { webkitRelativePath?: string }).webkitRelativePath || file.name;
    data.append('file', file, relativePath);
  }
  return data;
}

export const fileShareApi = {
  getSession() {
    return request<FileShareSession>('/api/session');
  },
  login(username: string, password: string) {
    return request<FileShareSession>('/api/auth/login', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({
        username,
        password,
      }),
    });
  },
  logout() {
    return request<void>('/api/auth/logout', {
      method: 'POST',
    }, 'empty');
  },
  getTree(nodeId?: string | null) {
    const query = new URLSearchParams();
    if (nodeId) {
      query.set('node_id', nodeId);
    }
    const suffix = query.size > 0 ? `?${query.toString()}` : '';
    return request<FileShareTreeResponse>(`/api/tree${suffix}`);
  },
  resolvePath(segments: string[]) {
    const path = segments.map((segment) => encodeURIComponent(segment)).join('/');
    const suffix = path.length > 0 ? `?path=${path}` : '';
    return request<FileShareResolveResponse>(`/api/resolve${suffix}`);
  },
  search(keyword: string, nodeId?: string | null) {
    const query = new URLSearchParams({
      keyword,
    });
    if (nodeId) {
      query.set('node_id', nodeId);
    }
    return request<FileShareSearchResponse>(`/api/tree/search?${query.toString()}`);
  },
  uploadFiles(parentNodeId: string, files: File[]) {
    return request<void>('/api/upload/files', {
      method: 'POST',
      body: buildMultipart(parentNodeId, files),
    }, 'empty');
  },
  uploadDirectory(parentNodeId: string, files: File[]) {
    return request<void>('/api/upload/directory', {
      method: 'POST',
      body: buildMultipart(parentNodeId, files),
    }, 'empty');
  },
  createDirectory(parentNodeId: string, name: string) {
    return request<void>('/api/nodes/directory', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({
        parent_node_id: parentNodeId,
        name,
      }),
    }, 'empty');
  },
  createText(parentNodeId: string, name: string, content: string) {
    return request<void>('/api/nodes/text', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({
        parent_node_id: parentNodeId,
        name,
        content,
      }),
    }, 'empty');
  },
  rename(nodeId: string, toName: string) {
    return request<void>('/api/nodes/rename', {
      method: 'PATCH',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({
        node_id: nodeId,
        to_name: toName,
      }),
    }, 'empty');
  },
  remove(nodeId: string) {
    return request<void>('/api/nodes', {
      method: 'DELETE',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({
        node_id: nodeId,
      }),
    }, 'empty');
  },
  previewUrl(nodeId: string) {
    const query = new URLSearchParams({
      node_id: nodeId,
    });
    return `/api/preview?${query.toString()}`;
  },
  downloadFileUrl(nodeId: string) {
    const query = new URLSearchParams({
      node_id: nodeId,
    });
    return `/api/download/file?${query.toString()}`;
  },
  downloadArchiveUrl(nodeId: string) {
    const query = new URLSearchParams({
      node_id: nodeId,
    });
    return `/api/download/archive?${query.toString()}`;
  },
};

export function isUnauthorized(error: unknown): boolean {
  return error instanceof FileShareApiError && error.status === 401;
}

export function isForbidden(error: unknown): boolean {
  return error instanceof FileShareApiError && error.status === 403;
}

export function isNotFound(error: unknown): boolean {
  return error instanceof FileShareApiError && error.status === 404;
}

export function getErrorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}
