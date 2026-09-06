use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::types::{CodingAgentError, CodingAgentResult};

static FILE_MUTATION_QUEUES: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> = OnceLock::new();

fn queues() -> &'static Mutex<HashMap<PathBuf, Arc<Mutex<()>>>> {
    FILE_MUTATION_QUEUES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn mutation_queue_key(file_path: &Path) -> CodingAgentResult<PathBuf> {
    let absolute_path = if file_path.is_absolute() {
        file_path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| CodingAgentError::File(format!("读取当前目录失败：{error}")))?
            .join(file_path)
    };

    match absolute_path.canonicalize() {
        Ok(real_path) => Ok(real_path),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::InvalidInput
            ) =>
        {
            Ok(absolute_path)
        }
        Err(error) => Err(CodingAgentError::File(format!(
            "解析文件路径 {} 失败：{error}",
            absolute_path.display()
        ))),
    }
}

pub fn with_file_mutation_queue<T>(
    file_path: &Path,
    operation: impl FnOnce() -> CodingAgentResult<T>,
) -> CodingAgentResult<T> {
    let key = mutation_queue_key(file_path)?;
    let queue = {
        let mut queues = queues()
            .lock()
            .map_err(|_| CodingAgentError::File("文件变更队列状态已损坏".to_string()))?;
        queues
            .entry(key.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    };

    // 锁作用域覆盖整个文件变更操作，确保同一路径不会并发读写交错。
    let _file_guard = queue
        .lock()
        .map_err(|_| CodingAgentError::File("文件变更队列锁已损坏".to_string()))?;
    let result = operation();

    let mut queues = queues()
        .lock()
        .map_err(|_| CodingAgentError::File("文件变更队列状态已损坏".to_string()))?;
    if Arc::strong_count(&queue) == 2 {
        queues.remove(&key);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn serializes_mutations_for_same_file() {
        let workspace = crate::tools::common::collect_temp_workspace("mutation-queue");
        fs::create_dir_all(&workspace).expect("create workspace");
        let path = workspace.join("same.txt");
        fs::write(&path, "start").expect("seed file");
        let (first_started_tx, first_started_rx) = mpsc::channel();
        let (release_first_tx, release_first_rx) = mpsc::channel();
        let first_path = path.clone();

        let first = thread::spawn(move || {
            with_file_mutation_queue(&first_path, || {
                first_started_tx.send(()).expect("send start");
                release_first_rx.recv().expect("wait release");
                fs::write(&first_path, "first").map_err(|error| {
                    CodingAgentError::File(format!("写入 {} 失败：{error}", first_path.display()))
                })?;
                Ok(())
            })
        });

        first_started_rx.recv().expect("first started");
        let second_path = path.clone();
        let second = thread::spawn(move || {
            with_file_mutation_queue(&second_path, || {
                fs::write(&second_path, "second").map_err(|error| {
                    CodingAgentError::File(format!("写入 {} 失败：{error}", second_path.display()))
                })?;
                Ok(())
            })
        });

        thread::sleep(Duration::from_millis(20));
        assert_eq!(
            fs::read_to_string(&path).expect("read while locked"),
            "start"
        );
        release_first_tx.send(()).expect("release first");
        first.join().expect("first join").expect("first ok");
        second.join().expect("second join").expect("second ok");

        assert_eq!(fs::read_to_string(path).expect("read final"), "second");
    }

    #[test]
    fn allows_mutations_for_different_files_to_run_in_parallel_like_pi() {
        let workspace = crate::tools::common::collect_temp_workspace("mutation-queue-parallel");
        fs::create_dir_all(&workspace).expect("create workspace");
        let first_path = workspace.join("first.txt");
        let second_path = workspace.join("second.txt");
        fs::write(&first_path, "start").expect("seed first file");
        fs::write(&second_path, "start").expect("seed second file");
        let (first_started_tx, first_started_rx) = mpsc::channel();
        let (second_started_tx, second_started_rx) = mpsc::channel();
        let (release_both_tx, release_both_rx) = mpsc::channel();

        let first = thread::spawn(move || {
            with_file_mutation_queue(&first_path, || {
                first_started_tx.send(()).expect("send first start");
                release_both_rx.recv().expect("wait release");
                fs::write(&first_path, "first").map_err(|error| {
                    CodingAgentError::File(format!("写入 {} 失败：{error}", first_path.display()))
                })?;
                Ok(())
            })
        });

        first_started_rx.recv().expect("first started");
        let second = thread::spawn(move || {
            with_file_mutation_queue(&second_path, || {
                second_started_tx.send(()).expect("send second start");
                fs::write(&second_path, "second").map_err(|error| {
                    CodingAgentError::File(format!("写入 {} 失败：{error}", second_path.display()))
                })?;
                Ok(())
            })
        });

        second_started_rx
            .recv_timeout(Duration::from_millis(100))
            .expect("second file should not wait for first file lock");
        release_both_tx.send(()).expect("release first");
        first.join().expect("first join").expect("first ok");
        second.join().expect("second join").expect("second ok");
    }

    #[cfg(unix)]
    #[test]
    fn serializes_mutations_for_symlink_aliases_like_pi() {
        let workspace = crate::tools::common::collect_temp_workspace("mutation-queue-symlink");
        fs::create_dir_all(&workspace).expect("create workspace");
        let target_path = workspace.join("target.txt");
        let symlink_path = workspace.join("alias.txt");
        fs::write(&target_path, "start").expect("seed file");
        std::os::unix::fs::symlink(&target_path, &symlink_path).expect("create symlink");
        let (first_started_tx, first_started_rx) = mpsc::channel();
        let (release_first_tx, release_first_rx) = mpsc::channel();
        let first_path = target_path.clone();

        let first = thread::spawn(move || {
            with_file_mutation_queue(&first_path, || {
                first_started_tx.send(()).expect("send start");
                release_first_rx.recv().expect("wait release");
                fs::write(&first_path, "first").map_err(|error| {
                    CodingAgentError::File(format!("写入 {} 失败：{error}", first_path.display()))
                })?;
                Ok(())
            })
        });

        first_started_rx.recv().expect("first started");
        let second_path = symlink_path.clone();
        let second = thread::spawn(move || {
            with_file_mutation_queue(&second_path, || {
                fs::write(&second_path, "second").map_err(|error| {
                    CodingAgentError::File(format!("写入 {} 失败：{error}", second_path.display()))
                })?;
                Ok(())
            })
        });

        thread::sleep(Duration::from_millis(20));
        assert_eq!(
            fs::read_to_string(&target_path).expect("read while locked"),
            "start"
        );
        release_first_tx.send(()).expect("release first");
        first.join().expect("first join").expect("first ok");
        second.join().expect("second join").expect("second ok");

        assert_eq!(
            fs::read_to_string(target_path).expect("read final"),
            "second"
        );
    }
}
