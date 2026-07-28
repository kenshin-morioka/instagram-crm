// テーマ切り替え (3ページ共通)。
// 描画前に data-theme を確定させるため、head内で同期読み込みすること。
(function () {
  var stored = null;
  try { stored = localStorage.getItem('theme'); } catch (e) { /* プライベートモード等 */ }
  var theme = stored === 'dark' || stored === 'light'
    ? stored
    : (window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light');
  document.documentElement.setAttribute('data-theme', theme);

  // 新規遷移でページ先頭から表示する。
  // ブラウザが遷移前のスクロール位置を引き継ぎ、リンク先が下端で開かれるのを防ぐ。
  // 戻る/進む (back_forward) とアンカー付きURLは対象外にして、本来の挙動を残す。
  function scrollToTopOnFreshNavigation() {
    if (location.hash) return;
    var entries = performance.getEntriesByType && performance.getEntriesByType('navigation');
    var navType = entries && entries.length ? entries[0].type : null;
    if (navType === 'back_forward' || navType === 'reload') return;
    // scroll-behavior: smooth のアニメーションを避けるため即座に移動する
    window.scrollTo({ top: 0, left: 0, behavior: 'instant' });
  }

  document.addEventListener('DOMContentLoaded', function () {
    scrollToTopOnFreshNavigation();

    var buttons = document.querySelectorAll('.theme-toggle');
    // スクリーンリーダー向けに現在のオン/オフ状態を公開する
    function syncButtons(current) {
      buttons.forEach(function (btn) {
        btn.setAttribute('aria-pressed', current === 'dark' ? 'true' : 'false');
      });
    }
    syncButtons(document.documentElement.getAttribute('data-theme'));

    buttons.forEach(function (btn) {
      btn.addEventListener('click', function () {
        var next = document.documentElement.getAttribute('data-theme') === 'dark' ? 'light' : 'dark';
        document.documentElement.setAttribute('data-theme', next);
        syncButtons(next);
        try { localStorage.setItem('theme', next); } catch (e) { /* 保存できなくても切り替えは有効 */ }
      });
    });
  });
})();
