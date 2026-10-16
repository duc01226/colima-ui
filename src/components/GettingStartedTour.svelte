<script lang="ts">
  import { onMount } from "svelte";
  import { isDockerOnlyHost } from "../store/capabilities.svelte";
  import { t } from "../lib/i18n.svelte";
  import TourTooltip from "./TourTooltip.svelte";

  let { onComplete } = $props<{
    onComplete: () => void;
  }>();

  interface TourStep {
    targetSelector: string;
    title: string;
    body: string;
    position?: "right" | "bottom" | "left";
  }

  const TOUR_STEPS: TourStep[] = [
    {
      targetSelector: '[data-tour-id="sidebar-nav"]',
      title: "Sidebar Navigation",
      body: "Navigate between features using the sidebar. It's organized into sections: Overview, Docker, Infrastructure, and Tools.",
      position: "right",
    },
    {
      targetSelector: '[data-tour-id="nav-dashboard"]',
      title: "Dashboard",
      body: "Monitor your VMs and Docker resources at a glance. See real-time stats for instances, containers, images, and more.",
      position: "right",
    },
    {
      targetSelector: '[data-tour-id="nav-instances"]',
      title: "Instance Management",
      body: "Create and manage Colima VM instances. Start, stop, restart, or delete instances with configurable resources.",
      position: "right",
    },
    {
      targetSelector: '[data-tour-id="nav-containers"]',
      title: "Docker Containers",
      body: "View and manage Docker containers. Run new containers, inspect logs, execute commands, and monitor resource usage.",
      position: "right",
    },
    {
      targetSelector: '[data-tour-id="nav-terminal"]',
      title: "Terminal Access",
      body: "Open a native terminal in a running container or another available target. Sessions stay open while you move between pages.",
      position: "right",
    },
    {
      targetSelector: '[data-tour-id="nav-dockerfile"]',
      title: "Dockerfile Generator",
      body: "Generate Dockerfiles with pre-built templates and AI assistance. Supports Node.js, Python, Go, Rust, and more.",
      position: "right",
    },
    {
      targetSelector: '[data-tour-id="nav-settings"]',
      title: "Settings & System",
      body: "Check system dependency status, view Docker disk usage, and manage application preferences.",
      position: "right",
    },
  ];

  let steps = $state<TourStep[]>([]);
  let currentStep = $state(0);
  let targetRect = $state<DOMRect | null>(null);
  let tooltipPos = $state({ left: 0, top: 0 });

  function updatePosition() {
    const step = steps[currentStep];
    if (!step) return;
    const el = document.querySelector(step.targetSelector);
    if (!el) return;

    const rect = el.getBoundingClientRect();
    targetRect = rect;

    // Position tooltip to the right of the target
    const tooltipWidth = 320;
    const tooltipHeight = 200;
    const gap = 16;
    let left = rect.right + gap;
    let top = rect.top;

    // If no room on right, position below
    if (left + tooltipWidth > window.innerWidth) {
      left = rect.left;
      top = rect.bottom + gap;
    }

    // Ensure tooltip stays in viewport
    if (top + tooltipHeight > window.innerHeight) {
      top = window.innerHeight - tooltipHeight - 20;
    }
    if (left < 10) left = 10;
    if (top < 10) top = 10;

    tooltipPos = { left, top };
  }

  $effect(() => {
    // Re-run when currentStep changes
    void currentStep;
    updatePosition();
  });

  onMount(() => {
    steps = TOUR_STEPS.filter((item) => document.querySelector(item.targetSelector)).map((item) => {
      if (!isDockerOnlyHost()) return item;
      const target = item.targetSelector.match(/data-tour-id="([^"]+)"/)?.[1] ?? "sidebar-nav";
      return { ...item, title: t(`connection.tour_${target}_title`), body: t(`connection.tour_${target}_body`) };
    });
    if (steps.length === 0) { onComplete(); return; }
    updatePosition();
    window.addEventListener("resize", updatePosition);
    return () => {
      window.removeEventListener("resize", updatePosition);
    };
  });

  $effect(() => {
    updatePosition();
  });

  function handleNext() {
    if (currentStep < steps.length - 1) {
      currentStep += 1;
    } else {
      onComplete();
    }
  }

  function handlePrev() {
    if (currentStep > 0) {
      currentStep -= 1;
    }
  }

  let step = $derived(steps[currentStep]);
  const padding = 6;

  let clipPath = $derived(
    targetRect
      ? `polygon(
          0% 0%, 0% 100%, 100% 100%, 100% 0%, 0% 0%,
          ${targetRect.left - padding}px ${targetRect.top - padding}px,
          ${targetRect.right + padding}px ${targetRect.top - padding}px,
          ${targetRect.right + padding}px ${targetRect.bottom + padding}px,
          ${targetRect.left - padding}px ${targetRect.bottom + padding}px,
          ${targetRect.left - padding}px ${targetRect.top - padding}px
        )`
      : "none"
  );
</script>

{#if step}
<div class="tour-overlay">
  <!-- Dark backdrop with spotlight hole -->
  <div
    class="tour-backdrop"
    style="clip-path: {clipPath};"
    onclick={handleNext}
  ></div>

  <!-- Highlight ring around target -->
  {#if targetRect}
    <div
      class="tour-highlight"
      style="left: {targetRect.left - padding}px; top: {targetRect.top - padding}px; width: {targetRect.width + padding * 2}px; height: {targetRect.height + padding * 2}px;"
    ></div>
  {/if}

  <!-- Tooltip -->
  <TourTooltip
    {step}
    {currentStep}
    totalSteps={steps.length}
    {tooltipPos}
    {onComplete}
    {handlePrev}
    {handleNext}
  />
</div>

{/if}
