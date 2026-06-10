import { createRouter, createWebHistory } from "vue-router";

export const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: "/", name: "tasks", component: () => import("./views/TasksList.vue") },
    { path: "/stats", name: "stats", component: () => import("./views/Stats.vue") },
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
      path: "/services",
      name: "services",
      component: () => import("./views/ServicesList.vue"),
    },
    {
      path: "/services/:id",
      name: "service-detail",
      component: () => import("./views/ServiceDetail.vue"),
      props: true,
    },
    {
      path: "/models",
      name: "models",
      component: () => import("./views/ModelsList.vue"),
    },
    {
      path: "/models/:id",
      name: "model-detail",
      component: () => import("./views/ModelDetail.vue"),
      props: true,
    },
    {
      path: "/providers",
      name: "providers",
      component: () => import("./views/ProvidersList.vue"),
    },
    {
      path: "/providers/:id",
      name: "provider-detail",
      component: () => import("./views/ProviderDetail.vue"),
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
