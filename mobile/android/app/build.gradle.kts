import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "com.otoko.mobile"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.otoko.mobile"
        minSdk = 26
        targetSdk = 35
        versionCode = 4
        versionName = "1.1.4"
    }

    // Toujours la même clé (sauvegardée dans android/cle, à garder) : les mises à jour
    // s'installent par-dessus. L'app Studio Vault garde la sienne : les deux cohabitent.
    // La clé et son mot de passe (cle/signature.properties) restent hors du dépôt Git : sans eux
    // (la vérification automatique sur GitHub), le code se compile quand même, sans signature.
    val signatureFile = rootProject.file("cle/signature.properties")
    if (signatureFile.exists()) {
        signingConfigs {
            create("otoko") {
                val signature = Properties().apply { signatureFile.inputStream().use { load(it) } }
                storeFile = rootProject.file("cle/otoko.keystore")
                storePassword = signature.getProperty("motdepasse")
                keyAlias = "otoko"
                keyPassword = signature.getProperty("motdepasse")
            }
        }
    }
    buildTypes {
        release {
            isMinifyEnabled = false
            if (signatureFile.exists()) signingConfig = signingConfigs.getByName("otoko")
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    buildFeatures { compose = true }
}

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2024.12.01")
    implementation(composeBom)
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.animation:animation")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.7")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.7")
    implementation("androidx.core:core-ktx:1.15.0")

    val media3 = "1.5.1"
    implementation("androidx.media3:media3-exoplayer:$media3")
    implementation("androidx.media3:media3-session:$media3")
    implementation("androidx.concurrent:concurrent-futures-ktx:1.2.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-guava:1.9.0")

    implementation("io.coil-kt:coil-compose:2.7.0")
}
