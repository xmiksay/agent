import { createRouter, createWebHashHistory } from "vue-router";

// Hash history so the static build is previewable from any path without a
// server rewrite. The live app uses createWebHistory.
export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: "/", name: "tasks", component: () => import("./views/TasksList.vue") },
    { path: "/tasks/:id", name: "task-detail", component: () => import("./views/TaskDetail.vue"), props: true },
    { path: "/projects", name: "projects", component: () => import("./views/ProjectsList.vue") },
    { path: "/projects/:id", name: "project-detail", component: () => import("./views/ProjectDetail.vue"), props: true },
    { path: "/git_services", name: "git-services", component: () => import("./views/GitServicesList.vue") },
    { path: "/git_services/:id", name: "git-service-detail", component: () => import("./views/GitServiceDetail.vue"), props: true },
    { path: "/auth_requests", name: "auth-queue", component: () => import("./views/AuthRequestsQueue.vue") },
    { path: "/auth_requests/:id", name: "auth-detail", component: () => import("./views/AuthRequestDetail.vue"), props: true },
    { path: "/stats", name: "stats", component: () => import("./views/Stats.vue") },
    { path: "/components", name: "components", component: () => import("./Workbench.vue") },
  ],
});
