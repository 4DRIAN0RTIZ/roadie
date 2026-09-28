(() => {
  const menuButton = document.querySelector('.menu-toggle');
  const nav = document.querySelector('#site-nav');

  if (menuButton && nav) {
    menuButton.addEventListener('click', () => {
      const open = menuButton.getAttribute('aria-expanded') === 'true';
      menuButton.setAttribute('aria-expanded', String(!open));
      nav.classList.toggle('is-open', !open);
    });

    nav.querySelectorAll('a').forEach((link) => {
      link.addEventListener('click', () => {
        menuButton.setAttribute('aria-expanded', 'false');
        nav.classList.remove('is-open');
      });
    });
  }

  document.querySelectorAll('[data-copy]').forEach((button) => {
    button.addEventListener('click', async () => {
      const original = button.textContent;
      try {
        await navigator.clipboard.writeText(button.dataset.copy);
        button.textContent = 'copied';
      } catch {
        button.textContent = 'select manually';
      }
      window.setTimeout(() => { button.textContent = original; }, 1600);
    });
  });

  const refresh = document.querySelector('.refresh-demo');
  if (refresh) {
    refresh.addEventListener('click', () => {
      const initial = refresh.textContent;
      refresh.textContent = 'fetching...';
      window.setTimeout(() => { refresh.textContent = 'up to date'; }, 700);
      window.setTimeout(() => { refresh.textContent = initial; }, 1900);
    });
  }
  const versionField = document.querySelector('#site-version');
  const updatedField = document.querySelector('#footer-updated');
  if (versionField || updatedField) {
    fetch('changelog.json')
      .then((res) => {
        if (!res.ok) throw new Error('changelog.json not found');
        return res.json();
      })
      .then((releases) => {
        const released = releases.find((r) => r.version);
        if (versionField && released) versionField.textContent = released.version;

        // Newest commit of the newest entry (unreleased work counts), else its release date.
        const latest = releases.find((r) => r.commits && r.commits.length) || released;
        const ts = latest?.commits?.[0]?.author?.timestamp || latest?.timestamp;
        if (updatedField && ts) {
          updatedField.textContent = `· Last updated: ${new Date(ts * 1000).toISOString().slice(0, 10)}`;
        }
      })
      .catch((error) => console.error('Error loading changelog.json:', error));
  }
})();
