// テーマ切り替え (3ページ共通)。
// 描画前に data-theme を確定させるため、head内で同期読み込みすること。
(function () {
  var stored = null;
  try { stored = localStorage.getItem('theme'); } catch (e) { /* プライベートモード等 */ }
  var theme = stored === 'dark' || stored === 'light'
    ? stored
    : (window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light');
  document.documentElement.setAttribute('data-theme', theme);

  document.addEventListener('DOMContentLoaded', function () {
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
