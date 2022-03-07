pipeline {
    agent any
     tools{
         nodejs "NodeJs"
     }

    stages {
        stage('install') {
            steps {
                sh 'npm install'
            }
        }
        stage('build') {
            steps {
                sh 'npm run build'
            }
        }
       
    }
}
