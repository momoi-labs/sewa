// Sets data-theme to light or dark before first paint. The saved Kiso theme
// wins. "system" or no value follows the operating system, also when it
// changes later (ThemeSelector).
(function () {
  var media = window.matchMedia('(prefers-color-scheme: dark)');
  function saved() {
    try {
      return localStorage.getItem('kiso-theme');
    } catch (_) {
      return null;
    }
  }
  function apply() {
    var theme = saved();
    if (theme !== 'light' && theme !== 'dark') theme = media.matches ? 'dark' : 'light';
    document.documentElement.dataset.theme = theme;
  }
  apply();
  media.addEventListener('change', apply);
})();
