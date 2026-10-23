//! Persist intent before receiving bytes, then seal a verified managed object.
use super::*;
use nw_domain::ingest::{UploadInput, UploadTicket};
use tokio::io::AsyncWriteExt;
const FILE_LIMIT:u64=1024*1024*1024;
const ROOT_LIMIT:u64=4*FILE_LIMIT;
#[derive(Clone, Serialize, Deserialize)]
struct Upload {
    id:String, principal:String, profile:Option<String>, display_name:String, expected:u64, expected_hash:Option<String>,
    expires:u64, phase:String, staging:String, sealed:Option<nw_domain::records::FileIdentity>,
}
pub struct UploadWriter {
    service:Arc<Service>, upload:Upload, file:Option<tokio::fs::File>, count:u64, hash:Sha256, completed:bool,
}
impl Drop for UploadWriter {
    fn drop(&mut self){
        self.file.take();
        if !self.completed {
            self.upload.phase="interrupted".into();
            let _=self.service.store.put(&format!("upload/{}",self.upload.id),&self.upload);
            let _=std::fs::remove_file(&self.upload.staging);
        }
    }
}
impl UploadWriter {
    pub async fn write(&mut self,chunk:&[u8])->Result<()> {
        if chunk.len()>1024*1024 || self.count.saturating_add(chunk.len() as u64)>self.upload.expected {return Err(DomainError::new("RESOURCE_LIMIT","Upload frame or declared length exceeded"));}
        let service=self.service.clone();let context=Context{principal:self.upload.principal.clone(),profile:self.upload.profile.clone()};
        tokio::task::spawn_blocking(move||service.upload_allowed(&context)).await.map_err(|_|DomainError::new("WORKER_FAILED","Upload authorization worker failed"))??;
        self.file.as_mut().ok_or_else(||DomainError::new("INGEST_NOT_READY","Writer is closed"))?.write_all(chunk).await.map_err(files::io_error)?;
        self.count+=chunk.len() as u64;self.hash.update(chunk);Ok(())
    }
    pub async fn finish(mut self)->Result<UploadTicket>{
        if self.count!=self.upload.expected {return Err(DomainError::new("INGEST_TRUNCATED","Upload did not match its declared byte length"));}
        let digest=hex::encode(self.hash.clone().finalize());
        if self.upload.expected_hash.as_ref().is_some_and(|hash|hash!=&digest){return Err(DomainError::new("VERIFICATION_FAILED","Upload digest differs from its declared digest"));}
        self.file.as_mut().ok_or_else(||DomainError::new("INGEST_NOT_READY","Writer closed"))?.sync_all().await.map_err(files::io_error)?;
        self.file.take();
        let service=self.service.clone();let upload=self.upload.clone();
        let sealed=tokio::task::spawn_blocking(move||service.seal_upload(&upload,&digest)).await.map_err(|_|DomainError::new("WORKER_FAILED","Upload seal worker failed"))??;
        self.upload=sealed;self.completed=true;
        Ok(UploadTicket {upload_id:self.upload.id.clone(),expected_bytes:self.upload.expected.to_string(),expires_at:self.upload.expires.to_string()})
    }
}
impl Service {
    fn upload_allowed(&self,context:&Context)->Result<()> {
        if let Some(profile)=self.policy(context)? && !profile.commands.contains("assets.register"){return Err(DomainError::new("PERMISSION_DENIED","Intake requires asset registration authority"));}Ok(())
    }
    pub fn create_upload(&self,context:&Context,input:UploadInput)->Result<UploadTicket>{
        self.upload_allowed(context)?;
        let _guard=self.mutation.lock().map_err(|_|lock_error())?;
        let expected=input.bytes.parse::<u64>().map_err(|_|DomainError::new("VALIDATION_ERROR","Upload bytes must be a canonical decimal string"))?;
        if expected==0 || expected>FILE_LIMIT || expected.to_string()!=input.bytes || input.display_name.is_empty() || input.display_name.len()>255 || input.display_name.chars().any(char::is_control) || input.sha256.as_ref().is_some_and(|hash|hash.len()!=64 || !hash.bytes().all(|byte|byte.is_ascii_hexdigit())){return Err(DomainError::new("VALIDATION_ERROR","Invalid filename, digest or declared upload length (maximum 1 GiB)"));}
        let now=nw_platform::epoch_seconds();let mut active=0;let mut count=0;let mut reserved=0u64;
        for (key,upload) in self.store.rows::<Upload>("upload/",10000)? {
            if upload.expires<=now {let _=std::fs::remove_file(&upload.staging);self.store.remove(&key)?;continue;}
            count+=1;
            if ["reserved","receiving"].contains(&upload.phase.as_str()){active+=1;reserved=reserved.saturating_add(upload.expected);}
        }
        let assets=nw_platform::security::private_directory(&self.root.join("assets"))?;
        for (index,entry) in std::fs::read_dir(&assets).map_err(files::io_error)?.enumerate(){
            if index>=4096{return Err(DomainError::new("RESOURCE_LIMIT","Managed object count quota reached"));}
            let entry=entry.map_err(files::io_error)?;let metadata=entry.metadata().map_err(files::io_error)?;
            if !metadata.is_file(){return Err(DomainError::new("PATH_DENIED","Unexpected managed object entry"));}reserved=reserved.saturating_add(metadata.len());
        }
        if active>=8 || count>=128 || reserved.saturating_add(expected)>ROOT_LIMIT{return Err(DomainError::new("RESOURCE_LIMIT","Intake count or 4 GiB managed storage quota reached"));}
        let staging=nw_platform::security::private_directory(&self.root.join("ingest"))?;
        let id=Uuid::new_v4().to_string();
        let upload=Upload{id:id.clone(),principal:context.principal.clone(),profile:context.profile.clone(),display_name:input.display_name,expected,expected_hash:input.sha256.map(|hash|hash.to_ascii_lowercase()),expires:now+3600,phase:"reserved".into(),staging:files::path_text(&staging.join(format!("{id}.part")))?,sealed:None};
        self.store.put(&named("upload",&id)?,&upload)?;
        Ok(UploadTicket{upload_id:id,expected_bytes:expected.to_string(),expires_at:upload.expires.to_string()})
    }
    pub fn begin_upload(self:&Arc<Self>,context:&Context,id:&str)->Result<UploadWriter>{
        self.upload_allowed(context)?;let _guard=self.mutation.lock().map_err(|_|lock_error())?;
        let key=named("upload",id)?;let mut upload:Upload=self.store.get(&key)?.ok_or_else(||DomainError::new("INGEST_NOT_FOUND","Intake handle does not exist"))?;
        if upload.principal!=context.principal || upload.expires<=nw_platform::epoch_seconds() || upload.phase!="reserved"{return Err(DomainError::new("INGEST_NOT_READY","Intake is expired, consumed or belongs to another principal"));}
        upload.phase="receiving".into();self.store.put(&key,&upload)?;
        nw_platform::security::write_private(Path::new(&upload.staging),&[],false)?;
        let file=std::fs::OpenOptions::new().write(true).open(&upload.staging).map_err(files::io_error)?;
        Ok(UploadWriter{service:self.clone(),upload,file:Some(tokio::fs::File::from_std(file)),count:0,hash:Sha256::new(),completed:false})
    }
    fn seal_upload(&self,upload:&Upload,digest:&str)->Result<Upload>{
        let context=Context{principal:upload.principal.clone(),profile:upload.profile.clone()};self.upload_allowed(&context)?;
        let extension=Path::new(&upload.display_name).extension().and_then(|value|value.to_str()).filter(|value|value.len()<=12 && value.bytes().all(|byte|byte.is_ascii_alphanumeric())).unwrap_or("bin").to_ascii_lowercase();
        let output=self.root.join("assets").join(format!("{digest}.{extension}"));
        match std::fs::hard_link(&upload.staging,&output){Ok(())=>{},Err(error) if error.kind()==std::io::ErrorKind::AlreadyExists=>{},Err(error)=>return Err(files::io_error(error))}
        let identity=files::identity(&output,&CancellationToken::new())?;
        if identity.bytes!=upload.expected.to_string() || identity.sha256!=digest{return Err(DomainError::new("VERIFICATION_FAILED","Managed object differs from the verified intake"));}
        #[cfg(unix)] std::fs::File::open(self.root.join("assets")).and_then(|file|file.sync_all()).map_err(files::io_error)?;
        let mut ready=upload.clone();ready.phase="ready".into();ready.sealed=Some(identity);self.store.put(&named("upload",&ready.id)?,&ready)?;
        std::fs::remove_file(&ready.staging).map_err(files::io_error)?;Ok(ready)
    }
    pub(super) fn resolve_upload(&self,context:&Context,id:&str)->Result<(PathBuf,String)>{
        self.upload_allowed(context)?;
        let upload:Upload=self.store.get(&named("upload",id)?)?.ok_or_else(||DomainError::new("INGEST_NOT_FOUND","Intake handle does not exist"))?;
        if upload.principal!=context.principal || upload.expires<=nw_platform::epoch_seconds() || upload.phase!="ready"{return Err(DomainError::new("INGEST_NOT_READY","Only this principal's verified, unexpired intake can be registered"));}
        let identity=upload.sealed.ok_or_else(||DomainError::new("INGEST_NOT_READY","Intake is not sealed"))?;files::revalidate(&identity,&CancellationToken::new())?;
        Ok((PathBuf::from(identity.path),upload.display_name))
    }
    pub(super) fn recover_uploads(&self)->Result<()> {
        for (key,mut upload) in self.store.rows::<Upload>("upload/",10000)? {
            if upload.phase=="receiving" || upload.expires<=nw_platform::epoch_seconds(){let _=std::fs::remove_file(&upload.staging);upload.phase="interrupted".into();self.store.put(&key,&upload)?;}
        }Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn streamed_intake_is_principal_scoped_and_only_exact_bytes_become_ready(){
        let root=tempfile::tempdir().unwrap();let service=Service::open(root.path().to_owned(),None).await.unwrap();
        let owner=service.owner();let browser=service.profile_context("browser").unwrap();let agent=service.profile_context("agent").unwrap();
        let input=UploadInput{display_name:"../../label.mkv".into(),bytes:"6".into(),sha256:Some(hex::encode(Sha256::digest(b"abcdef")))};
        assert!(service.create_upload(&agent,input.clone()).is_err());
        let ticket=service.create_upload(&browser,input.clone()).unwrap();
        assert!(service.begin_upload(&owner,&ticket.upload_id).is_err());
        let mut writer=service.begin_upload(&browser,&ticket.upload_id).unwrap();writer.write(b"abc").await.unwrap();writer.write(b"def").await.unwrap();writer.finish().await.unwrap();
        let (path,label)=service.resolve_upload(&browser,&ticket.upload_id).unwrap();assert_eq!(label,"../../label.mkv");assert!(path.starts_with(root.path().join("assets")));assert_eq!(std::fs::read(path).unwrap(),b"abcdef");assert!(service.resolve_upload(&owner,&ticket.upload_id).is_err());assert!(service.begin_upload(&browser,&ticket.upload_id).is_err());
        let truncated=service.create_upload(&browser,input.clone()).unwrap();let mut writer=service.begin_upload(&browser,&truncated.upload_id).unwrap();writer.write(b"abc").await.unwrap();assert!(writer.finish().await.is_err());assert!(service.resolve_upload(&browser,&truncated.upload_id).is_err());
        let wrong=service.create_upload(&browser,input).unwrap();let mut writer=service.begin_upload(&browser,&wrong.upload_id).unwrap();writer.write(b"abcdeg").await.unwrap();assert!(writer.finish().await.is_err());assert!(service.resolve_upload(&browser,&wrong.upload_id).is_err());
    }
}
