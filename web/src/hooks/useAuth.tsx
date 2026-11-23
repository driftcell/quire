import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { useQueryClient } from "@tanstack/react-query";
import { fetchMe, login as apiLogin, registerAccount } from "@/api/auth";
import {
  clearAuthSession,
  getAuthSession,
  subscribeAuthSession,
  type AuthSession,
  type AuthUser,
} from "@/lib/auth";

interface AuthContextValue {
  session: AuthSession | null;
  user: AuthUser | null;
  isAuthenticated: boolean;
  isAdmin: boolean;
  isInitializing: boolean;
  login: (identifier: string, password: string) => Promise<AuthSession>;
  register: (payload: {
    username: string;
    email?: string;
    display_name?: string;
    password: string;
  }) => Promise<AuthSession>;
  logout: () => void;
}

const AuthContext = createContext<AuthContextValue | null>(null);

export function AuthProvider({ children }: { children: ReactNode }) {
  const queryClient = useQueryClient();
  const [session, setSession] = useState<AuthSession | null>(() =>
    getAuthSession(),
  );
  const [isInitializing, setIsInitializing] = useState(() =>
    Boolean(getAuthSession()),
  );

  useEffect(() => {
    const unsub = subscribeAuthSession(() => {
      setSession(getAuthSession());
    });
    return unsub;
  }, []);

  useEffect(() => {
    const existing = getAuthSession();
    if (!existing) {
      return undefined;
    }
    let cancelled = false;
    void fetchMe()
      .catch(() => {
        // 401 clears stored session automatically; network errors keep cached user.
      })
      .finally(() => {
        if (!cancelled) {
          setSession(getAuthSession());
          setIsInitializing(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const login = useCallback(
    async (identifier: string, password: string) => {
      const next = await apiLogin(identifier, password);
      setSession(next);
      queryClient.clear();
      return next;
    },
    [queryClient],
  );

  const register = useCallback(
    async (payload: {
      username: string;
      email?: string;
      display_name?: string;
      password: string;
    }) => {
      const next = await registerAccount(payload);
      setSession(next);
      queryClient.clear();
      return next;
    },
    [queryClient],
  );

  const logout = useCallback(() => {
    clearAuthSession();
    setSession(null);
    queryClient.clear();
  }, [queryClient]);

  const value = useMemo<AuthContextValue>(
    () => ({
      session,
      user: session?.user ?? null,
      isAuthenticated: Boolean(session?.token),
      isAdmin: session?.user.role === "admin",
      isInitializing,
      login,
      register,
      logout,
    }),
    [isInitializing, login, logout, register, session],
  );

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

export function useAuth(): AuthContextValue {
  const ctx = useContext(AuthContext);
  if (!ctx) {
    throw new Error("useAuth must be used inside AuthProvider");
  }
  return ctx;
}
