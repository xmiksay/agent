import { createRouter, createWebHistory } from "vue-router";

export const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: "/", name: "tasks", component: () => import("./views/TasksList.vue") },
    {
      path: "/tasks/:id",
      name: "task-detail",
      component: () => import("./views/TaskDetail.vue"),
      props: true,
    },
    { path: "/projects", name: "projects", component: () => import("./views/ProjectsList.vue") },
    {
      path: "/projects/:id",
      name: "project-detail",
      component: () => import("./views/ProjectDetail.vue"),
      props: true,
    },
    {
      path: "/git_services",
      name: "git-services",
      component: () => import("./views/GitServicesList.vue"),
    },
    {
      path: "/git_services/:id",
      name: "git-service-detail",
      component: () => import("./views/GitServiceDetail.vue"),
      props: true,
    },
    {
      path: "/auth_requests",
      name: "auth-queue",
      component: () => import("./views/AuthRequestsQueue.vue"),
    },
    {
      path: "/auth_requests/:id",
      name: "auth-detail",
      component: () => import("./views/AuthRequestDetail.vue"),
      props: true,
    },
  ],
});
