<script lang="ts">
  import "../app.css";
  import { onMount } from "svelte";
  import { page } from "$app/stores";
  import { goto } from "$app/navigation";
  import { getSettings } from "$lib/api";

  onMount(() => {
    let cancelled = false;
    void (async () => {
      try {
        const settings = await getSettings();
        if (cancelled) return;
        if (!settings.onboarding.completed && $page.url.pathname !== "/onboarding") {
          await goto("/onboarding");
        }
      } catch {
        // If settings fail to load, stay on the current route.
      }
    })();
    return () => {
      cancelled = true;
    };
  });
</script>

<slot />
