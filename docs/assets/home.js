(() => {
    const dialog = document.querySelector('.image-viewer');
    if (!dialog || typeof dialog.showModal !== 'function') return;
    const image = dialog.querySelector('img');
    const caption = dialog.querySelector('p');
    const close = dialog.querySelector('.viewer-close');
    let origin;
    document.querySelectorAll('[data-zoom]').forEach(link => link.addEventListener('click', event => {
        event.preventDefault();
        origin = link;
        image.src = link.href;
        image.alt = link.querySelector('img')?.alt || '界面截图';
        caption.textContent = image.alt;
        dialog.showModal();
    }));
    close.addEventListener('click', () => dialog.close());
    dialog.addEventListener('click', event => { if (event.target === dialog) dialog.close(); });
    dialog.addEventListener('close', () => { image.removeAttribute('src'); origin?.focus(); });
})();
