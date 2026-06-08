import { createRootRoute, createRouter, RouterProvider } from "@tanstack/react-router";
import { WorkspaceApp } from "./WorkspaceApp";

const rootRoute = createRootRoute({
  component: WorkspaceApp,
});

export const router = createRouter({
  routeTree: rootRoute,
});

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}

export function AppRouter() {
  return <RouterProvider router={router} />;
}
