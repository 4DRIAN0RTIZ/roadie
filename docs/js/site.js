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
})();
