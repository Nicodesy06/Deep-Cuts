// Deep Cuts site: navigation, guide table of contents, and download buttons
// filled from the latest GitHub release (falls back to the releases page).
(() => {
  const REPO = "Nicodesy06/Deep-Cuts";
  const RELEASES = `https://github.com/${REPO}/releases`;

  // Mobile navigation.
  const toggle = document.querySelector(".nav-toggle");
  const nav = document.getElementById("site-nav");
  if (toggle && nav) {
    toggle.addEventListener("click", () => {
      const open = nav.classList.toggle("open");
      toggle.setAttribute("aria-expanded", String(open));
    });
    nav.addEventListener("click", (e) => {
      if (e.target.closest("a")) {
        nav.classList.remove("open");
        toggle.setAttribute("aria-expanded", "false");
      }
    });
  }

  document.querySelectorAll("[data-year]").forEach((el) => (el.textContent = String(new Date().getFullYear())));

  // Guide: highlight the section in view.
  const tocLinks = [...document.querySelectorAll(".toc a[href^='#']")];
  if (tocLinks.length && "IntersectionObserver" in window) {
    const byId = new Map(tocLinks.map((a) => [a.getAttribute("href").slice(1), a]));
    const io = new IntersectionObserver(
      (entries) => {
        for (const e of entries) {
          if (!e.isIntersecting) continue;
          tocLinks.forEach((a) => a.classList.remove("active"));
          const a = byId.get(e.target.id);
          if (a) a.classList.add("active");
        }
      },
      { rootMargin: "-20% 0px -70% 0px" },
    );
    byId.forEach((_, id) => {
      const s = document.getElementById(id);
      if (s) io.observe(s);
    });
  }

  // ---------- downloads ----------
  const dlTargets = document.querySelectorAll("[data-dl]");
  const versionTargets = document.querySelectorAll("[data-version]");
  const dateTargets = document.querySelectorAll("[data-date]");
  const notesTargets = document.querySelectorAll("[data-notes]");
  const primary = document.querySelectorAll("[data-primary-dl]");
  if (!dlTargets.length && !primary.length && !versionTargets.length) return;

  function detectOS() {
    const ua = navigator.userAgent || "";
    const plat = (navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || "";
    if (/iPhone|iPad|iPod|Android/i.test(ua)) return "mobile";
    if (/Win/i.test(plat) || /Windows/i.test(ua)) return "windows";
    if (/Mac/i.test(plat) || /Mac OS X/i.test(ua)) return "mac";
    if (/Linux|X11/i.test(plat) || /Linux/i.test(ua)) return "linux";
    return "unknown";
  }
  const os = detectOS();
  document.querySelectorAll(`.dl-card[data-os="${os}"]`).forEach((c) => c.classList.add("current"));

  // Asset patterns produced by the release workflow (tauri-action).
  const PATTERNS = {
    "mac-arm": /_aarch64\.dmg$/i,
    "mac-intel": /_x64\.dmg$/i,
    "win-exe": /_x64-setup\.exe$/i,
    "win-msi": /_x64_[^.]*\.msi$/i,
    "linux-deb": /_amd64\.deb$/i,
    "linux-appimage": /_amd64\.AppImage$/i,
  };
  const PRIMARY = {
    mac: { key: "mac-arm", label: "Download for macOS" },
    windows: { key: "win-exe", label: "Download for Windows" },
    linux: { key: "linux-deb", label: "Download for Linux" },
  };

  const mb = (bytes) => `${Math.round(bytes / 1048576)} MB`;
  const fmtDate = (iso) =>
    new Date(iso).toLocaleDateString(undefined, { year: "numeric", month: "long", day: "numeric" });

  function setPrimary(release, assets) {
    primary.forEach((el) => {
      const p = PRIMARY[os];
      const textEl = el.querySelector("[data-primary-label]");
      const metaEl = el.querySelector(".meta");
      if (p && assets[p.key]) {
        el.href = assets[p.key].browser_download_url;
        if (textEl) textEl.textContent = p.label;
        if (metaEl) metaEl.textContent = `${release.tag_name} · ${mb(assets[p.key].size)}`;
      } else {
        el.href = "#download";
        if (textEl) textEl.textContent = "Download";
        if (metaEl) metaEl.textContent = release ? release.tag_name : "";
      }
    });
  }

  function fallback() {
    dlTargets.forEach((el) => {
      el.href = RELEASES;
      const m = el.querySelector(".meta");
      if (m) m.textContent = "GitHub";
    });
    primary.forEach((el) => {
      const p = PRIMARY[os];
      el.href = p ? RELEASES : "#download";
      const t = el.querySelector("[data-primary-label]");
      if (t && p) t.textContent = p.label;
    });
    versionTargets.forEach((el) => (el.textContent = "latest"));
  }

  fetch(`https://api.github.com/repos/${REPO}/releases?per_page=15`, {
    headers: { Accept: "application/vnd.github+json" },
  })
    .then((r) => (r.ok ? r.json() : Promise.reject(new Error(String(r.status)))))
    .then((list) => {
      // App releases are tagged vX.Y.Z; model-file releases are skipped.
      const release = list.find(
        (r) => !r.draft && /^v\d/.test(r.tag_name) && r.assets.some((a) => /\.(dmg|exe|msi|deb|AppImage)$/i.test(a.name)),
      );
      if (!release) return fallback();
      const assets = {};
      for (const [key, re] of Object.entries(PATTERNS)) {
        const a = release.assets.find((x) => re.test(x.name));
        if (a) assets[key] = a;
      }
      versionTargets.forEach((el) => (el.textContent = release.tag_name));
      dateTargets.forEach((el) => (el.textContent = fmtDate(release.published_at || release.created_at)));
      notesTargets.forEach((el) => (el.href = release.html_url));
      dlTargets.forEach((el) => {
        const a = assets[el.dataset.dl];
        const m = el.querySelector(".meta");
        if (a) {
          el.href = a.browser_download_url;
          if (m) m.textContent = mb(a.size);
          el.removeAttribute("aria-disabled");
        } else {
          el.href = release.html_url;
          if (m) m.textContent = "not in this release";
        }
      });
      setPrimary(release, assets);
    })
    .catch(fallback);
})();
