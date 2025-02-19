import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';

const root = ReactDOM.createRoot(document.getElementById('root'));
root.render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);

import styled, { createGlobalStyle } from 'styled-components';

const GlobalStyle = createGlobalStyle`
  :root {
    --bg: #0f0f0f;
    --card-bg: #1a1a1a;
    --text: #e0e0e0;
    --accent: #2b7a78;
    --hover: #3daaaa;
  }

  * {
    margin: 0;
    padding: 0;
    box-sizing: border-box;
  }

  body {
    background: var(--bg);
    color: var(--text);
    line-height: 1.6;
    font-family: 'Inter', system-ui, sans-serif;
    padding: 2rem 1rem;
  }
`;

const Container = styled.div`
  max-width: 1200px;
  margin: 0 auto;
`;

const Header = styled.header`
  text-align: center;
  margin-bottom: 3rem;

  h1 {
    color: var(--hover);
    font-size: 2.5rem;
    margin-bottom: 1rem;
  }
`;

const ButtonGroup = styled.div`
  display: flex;
  gap: 1rem;
  justify-content: center;
  margin: 2rem 0;

  @media (max-width: 768px) {
    flex-direction: column;
  }
`;

const Button = styled.a`
  padding: 0.8rem 1.5rem;
  border-radius: 6px;
  text-decoration: none;
  font-weight: 500;
  transition: 0.2s all ease;
  cursor: pointer;

  &.primary {
    background: var(--accent);
    color: white;

    &:hover {
      background: var(--hover);
    }
  }

  &.secondary {
    border: 1px solid var(--accent);
    color: var(--accent);

    &:hover {
      border-color: var(--hover);
      color: var(--hover);
    }
  }
`;

const Section = styled.section`
  background: var(--card-bg);
  border-radius: 8px;
  padding: 2rem;
  margin-bottom: 2rem;
`;

const FeatureGrid = styled.div`
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(250px, 1fr));
  gap: 1rem;
  margin-top: 1.5rem;
`;

const Feature = styled.div`
  padding: 1rem;
  background: var(--bg);
  border-radius: 6px;
`;

const ScreenshotGrid = styled.div`
  display: grid;
  gap: 1rem;
  margin-top: 2rem;

  img {
    width: 100%;
    border-radius: 6px;
    border: 1px solid #333;
  }
`;

const IssuesList = styled.ul`
  color: #ff6666;
  list-style-position: inside;
`;

const Credits = styled.footer`
  opacity: 0.8;
  font-size: 0.9rem;
`;

const App = () => {
  return (
    <>
      <GlobalStyle />
      <Container>
        <Header>
          <h1>Roblox Macro Utilities</h1>
          <p>Open-source Windows utility for Roblox automation with sensible defaults</p>
          
          <ButtonGroup>
            <Button 
              href="https://github.com/Spencer0187/Roblox-Macro-Utilities/releases/latest" 
              className="primary"
            >
              Download Latest Version
            </Button>
            <Button 
              href="https://github.com/Spencer0187/Roblox-Macro-Utilities" 
              className="secondary"
            >
              View Source Code
            </Button>
          </ButtonGroup>
        </Header>

        <Section>
          <h2>What It Does</h2>
          <p>A C++ program that helps automate various Roblox actions through macros and quality-of-life features. Works externally without touching Roblox memory.</p>
          
          <FeatureGrid>
            <Feature>
              <h3>Core Features</h3>
              <ul>
                <li>Persistent anti-AFK</li>
                <li>Customizable UI layout</li>
                <li>Automatic settings save</li>
                <li>15+ gameplay macros</li>
              </ul>
            </Feature>
            
            <Feature>
              <h3>Key Technical Stuff</h3>
              <ul>
                <li>Windows-only (for now)</li>
                <li>Requires VS 2022 C++ Redist</li>
                <li>No install needed</li>
                <li>Open source</li>
              </ul>
            </Feature>
          </FeatureGrid>
        </Section>

        <Section>
          <h2>In Action</h2>
          <ScreenshotGrid>
            {/* Replace with actual image imports */}
            <img src="path/to/screenshot1.jpg" alt="UI customization preview" />
            <img src="path/to/screenshot2.jpg" alt="Feature demonstration" />
          </ScreenshotGrid>
        </Section>

        <Section>
          <h2>Common Fixes</h2>
          <IssuesList>
            <li><strong>DLL errors:</strong> Install <a href="https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist">VS 2022 x64 Redist</a></li>
            <li><strong>0x0 key errors:</strong> Full system restart</li>
            <li><strong>Launch issues:</strong> Right-click → Properties → Unblock</li>
          </IssuesList>
        </Section>

        <Section>
          <h2>Need Help?</h2>
          <p>Join the community support Discord:</p>
          <Button 
            href="https://discord.gg/roblox-glitching-community-998572881892094012" 
            className="secondary"
          >
            Roblox Glitching Discord
          </Button>
        </Section>

        <Credits>
          <p>Built using:</p>
          <ul>
            <li><a href="https://github.com/ocornut/imgui">ImGui</a> for the interface</li>
            <li><a href="https://github.com/craftwar/suspend">suspend</a> framework</li>
          </ul>
        </Credits>
      </Container>
    </>
  );
};

export default App;
