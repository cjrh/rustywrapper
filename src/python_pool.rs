use pyo3::prelude::*;
use pyo3::types::PyDict;

pub struct PyProcessPool {
    executor: Py<PyAny>,
    worker_module: Py<PyAny>,
}

unsafe impl Send for PyProcessPool {}
unsafe impl Sync for PyProcessPool {}

impl PyProcessPool {
    pub fn new(max_workers: usize) -> PyResult<Self> {
        Python::attach(|py| {
            let sys = py.import("sys")?;
            let path = sys.getattr("path")?;
            path.call_method1("insert", (0, "python"))?;

            let worker_module = py.import("pool_workers")?;
            let initializer = worker_module.getattr("init_worker")?;

            let concurrent_futures = py.import("concurrent.futures")?;
            let kwargs = PyDict::new(py);
            kwargs.set_item("max_workers", max_workers)?;
            kwargs.set_item("initializer", initializer)?;

            let executor = concurrent_futures
                .getattr("ProcessPoolExecutor")?
                .call((), Some(&kwargs))?
                .into_any()
                .unbind();

            let worker_module = worker_module.into_any().unbind();

            Ok(Self {
                executor,
                worker_module,
            })
        })
    }

    pub fn compute_squares(&self, numbers: Vec<i64>) -> PyResult<Vec<i64>> {
        Python::attach(|py| {
            let executor = self.executor.bind(py);
            let worker_module = self.worker_module.bind(py);

            let compute_squares_fn = worker_module.getattr("compute_squares")?;
            let future = executor.call_method1("submit", (compute_squares_fn, numbers))?;
            let result = future.call_method0("result")?;

            result.extract::<Vec<i64>>()
        })
    }

    pub fn shutdown(&self) -> PyResult<()> {
        Python::attach(|py| {
            let executor = self.executor.bind(py);
            let kwargs = PyDict::new(py);
            kwargs.set_item("wait", true)?;
            kwargs.set_item("cancel_futures", true)?;
            match executor.call_method("shutdown", (), Some(&kwargs)) {
                Ok(_) => Ok(()),
                Err(e) if e.is_instance_of::<pyo3::exceptions::PyKeyboardInterrupt>(py) => Ok(()),
                Err(e) => Err(e),
            }
        })
    }
}

impl Drop for PyProcessPool {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}
