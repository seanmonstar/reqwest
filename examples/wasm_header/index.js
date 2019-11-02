const rust = import('./pkg');

rust
    .then(m => {
        return m.run().then((data) => {
            console.log(data);
        })
    })
    .catch(console.error);