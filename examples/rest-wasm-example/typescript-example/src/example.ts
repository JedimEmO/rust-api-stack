import {
  getUsers,
  getUsersId,
  getUsersUserIdTasks,
  postUsers,
} from './generated';
import type { CreateUserRequest, Task, User } from './generated';

const BASE_URL = 'http://localhost:3000/api/v1';

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

export async function listUsers(baseUrl = BASE_URL): Promise<User[]> {
  const response = await getUsers({ baseUrl });
  return requireData('GET /users', response).users;
}

export async function getUser(userId: string, baseUrl = BASE_URL): Promise<User> {
  const response = await getUsersId({
    baseUrl,
    path: { id: userId },
  });

  return requireData('GET /users/{id}', response);
}

export async function createUser(
  request: CreateUserRequest,
  adminToken: string,
  baseUrl = BASE_URL,
): Promise<User> {
  const response = await postUsers({
    baseUrl,
    headers: {
      Authorization: `Bearer ${adminToken}`,
    },
    body: request,
  });

  return requireData('POST /users', response);
}

export async function listUserTasks(
  userId: string,
  bearerToken: string,
  baseUrl = BASE_URL,
): Promise<Task[]> {
  const response = await getUsersUserIdTasks({
    baseUrl,
    headers: {
      Authorization: `Bearer ${bearerToken}`,
    },
    path: { user_id: userId },
  });

  return requireData('GET /users/{user_id}/tasks', response).tasks;
}
