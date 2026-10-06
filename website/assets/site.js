/* BRIDGE public site — shared script.
   Vanilla only: mobile nav toggle, copy-to-clipboard buttons,
   active-nav highlighting. No dependencies, no build step. */
(function () {
  "use strict";

  function ready(fn) {
    if (document.readyState !== "loading") {
      fn();
    } else {
      document.addEventListener("DOMContentLoaded", fn);
    }
  }

  function copyText(text) {
    if (navigator.clipboard && window.isSecureContext) {
      return navigator.clipboard.writeText(text);
    }
    return new Promise(function (resolve, reject) {
      var ta = document.createElement("textarea");
      ta.value = text;
      ta.setAttribute("readonly", "");
      ta.style.position = "absolute";
      ta.style.left = "-9999px";
      document.body.appendChild(ta);
      ta.select();
      try {
        var ok = document.execCommand("copy");
        document.body.removeChild(ta);
        if (ok) {
          resolve();
        } else {
          reject(new Error("copy command failed"));
        }
      } catch (e) {
        document.body.removeChild(ta);
        reject(e);
      }
    });
  }

  function flashButton(btn, message) {
    if (!btn.hasAttribute("data-label")) {
      btn.setAttribute("data-label", btn.textContent);
    }
    btn.textContent = message;
    btn.classList.add("copied");
    window.setTimeout(function () {
      btn.textContent = btn.getAttribute("data-label");
      btn.classList.remove("copied");
    }, 1600);
  }

  function initCopyButtons() {
    var buttons = document.querySelectorAll("[data-copy], [data-copy-target]");
    Array.prototype.forEach.call(buttons, function (btn) {
      btn.addEventListener("click", function () {
        var text = btn.getAttribute("data-copy");
        if (!text) {
          var selector = btn.getAttribute("data-copy-target");
          var target = selector ? document.querySelector(selector) : null;
          if (!target) {
            return;
          }
          text = target.textContent || "";
        }
        copyText(text).then(
          function () {
            flashButton(btn, "Copied");
          },
          function () {
            flashButton(btn, "Failed");
          }
        );
      });
    });
  }

  function initNavToggle() {
    var toggle = document.querySelector(".nav-toggle");
    var header = document.querySelector(".site-header");
    if (!toggle || !header) {
      return;
    }
    toggle.addEventListener("click", function () {
      var isOpen = header.classList.toggle("nav-open");
      toggle.setAttribute("aria-expanded", isOpen ? "true" : "false");
    });
  }

  function initActiveNav() {
    var path = window.location.pathname;
    if (path.charAt(path.length - 1) === "/") {
      path += "index.html";
    }
    var links = document.querySelectorAll(".site-header nav a, .site-footer nav a");
    Array.prototype.forEach.call(links, function (link) {
      var linkPath;
      try {
        linkPath = new window.URL(link.href, window.location.href).pathname;
      } catch (e) {
        return;
      }
      if (linkPath.charAt(linkPath.length - 1) === "/") {
        linkPath += "index.html";
      }
      if (linkPath === path) {
        link.setAttribute("aria-current", "page");
      }
    });
    if (path.indexOf("/docs/") !== -1) {
      var parents = document.querySelectorAll(".site-header .nav-docs-parent");
      Array.prototype.forEach.call(parents, function (link) {
        link.classList.add("nav-section-active");
      });
    }
  }

  ready(function () {
    initNavToggle();
    initCopyButtons();
    initActiveNav();
  });
})();
